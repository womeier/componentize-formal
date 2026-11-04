//! Generate a Rocq interface, a CertiRocq extraction file and canonical-ABI
//! glue from a WIT world.
//!
//! `check` decides whether a world is within the subset the backend can
//! honour, and points at `TODO.md` for everything it turns away. `generate`
//! runs the same gate and then writes the files, so nothing is ever emitted
//! for a world the gate would reject.

use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, ensure, Context, Result};
use clap::{Args, Parser, Subcommand};
use componentize_formal::{config::Config, emit, support};
use wit_parser::{Resolve, WorldId};

#[derive(Parser)]
#[command(name = "componentize-formal", version, about, max_term_width = 100)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Report whether a world is within the supported subset.
    Check(Input),
    /// Write interface.v, extraction.v, adapter.wat and the build rules.
    Generate {
        #[command(flatten)]
        input: Input,
        /// Overwrite output files even if they carry no generated marker.
        ///
        /// Without it, a file that may be hand-written stops the run before
        /// anything is written.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Args)]
struct Input {
    /// A component.toml. Without it, pass --wit.
    #[arg(long, short = 'c', value_name = "PATH")]
    config: Option<PathBuf>,
    /// A WIT file or directory, overriding the config.
    #[arg(long, value_name = "PATH")]
    wit: Option<PathBuf>,
    /// Which world to select, overriding the config.
    #[arg(long, value_name = "NAME")]
    world: Option<String>,
    /// Where to write the generated files, overriding the config. Has to be
    /// the directory holding your implementation file.
    #[arg(long, value_name = "PATH")]
    out_dir: Option<PathBuf>,
    /// Which proof assistant the implementation is written in. Overrides the
    /// config. Only `rocq` is implemented; see TODO.md#frontends.
    #[arg(long, value_enum)]
    frontend: Option<FrontendArg>,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum FrontendArg {
    /// A Rocq `Module Type`, compiled by `CertiRocq Compile Wasm`.
    Rocq,
    /// Not implemented yet.
    Lean,
}

impl From<FrontendArg> for componentize_formal::config::Frontend {
    fn from(arg: FrontendArg) -> Self {
        use componentize_formal::config::Frontend;
        match arg {
            FrontendArg::Rocq => Frontend::Rocq,
            FrontendArg::Lean => Frontend::Lean,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Check(input) => check(input),
        Cmd::Generate { input, force } => generate(input, *force),
    }
}

fn check(input: &Input) -> Result<ExitCode> {
    let w = World::load(input)?;
    let report = w.gate();

    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    for rejection in &report.rejections {
        eprintln!("{rejection}");
    }

    if !report.is_ok() {
        eprintln!(
            "\n{} unsupported construct(s) in world `{}`",
            report.rejections.len(),
            w.name()
        );
        return Ok(ExitCode::FAILURE);
    }
    println!("ok: world `{}` is within the supported subset", w.name());
    Ok(ExitCode::SUCCESS)
}

fn generate(input: &Input, force: bool) -> Result<ExitCode> {
    let w = World::load(input)?;
    let report = w.gate();

    // Warnings describe narrowings the generated adapter has to guard at
    // runtime, so they are never suppressed on the way to output.
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    if !report.is_ok() {
        for rejection in &report.rejections {
            eprintln!("{rejection}");
        }
        bail!(
            "world `{}` is outside the supported subset; nothing generated",
            w.name()
        );
    }

    let out_dir = &w.cfg.out_dir;
    // Before anything is resolved against it: a path is only canonicalisable
    // once it exists, and `--out-dir gen` naming a directory that is not there
    // yet is the ordinary case rather than a mistake.
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating the output directory {}", out_dir.display()))?;
    let wit_dir = w.wit_dir()?;
    let relative_wit = relative(&wit_dir, out_dir).with_context(|| {
        format!(
            "the generated build rules have to name the WIT directory {} from {}",
            wit_dir.display(),
            out_dir.display()
        )
    })?;

    // `wasm-tools component embed` reads a directory, so a world given as a
    // single file is built from whatever else sits beside it.
    if w.cfg.wit.is_file() {
        eprintln!(
            "note: the generated build rules read the whole of {}, not just {}, \
             because that is what `wasm-tools component embed` resolves a world \
             from",
            wit_dir.display(),
            w.cfg.wit.display(),
        );
    }

    // The generated `interface.v` is required by your implementation and
    // `extraction.v` requires it back, so Rocq has to find all three on one
    // load path. Putting them in different directories produces a build that
    // fails in Rocq rather than anything this tool could explain.
    let impl_dir = w.cfg.impl_.file.parent().unwrap_or(Path::new("."));
    ensure!(
        same_dir(impl_dir, out_dir),
        "the output directory is {} but the implementation file is {}. They have to \
         sit together: `{}` requires the generated interface and the generated \
         extraction file requires `{}` back, and Rocq resolves both by load path.",
        out_dir.display(),
        w.cfg.impl_.file.display(),
        w.cfg.impl_.file.display(),
        w.cfg.impl_.file.display(),
    );

    let plan = emit::Plan::new(&w.resolve, w.world, &w.cfg, &relative_wit)?;
    let mut artifacts = emit::artifacts(&plan);

    // The WIT directory is an input. Vendoring the error package into it is
    // setting up a project when it is part of one, and writing into someone
    // else's tree when it is not -- so outside the output directory this says
    // what is missing instead of fixing it.
    let abi = emit::vendored_abi(&plan);
    if relative_wit.starts_with("..") {
        let path = wit_dir.join(abi.path.strip_prefix(&relative_wit).unwrap_or(&abi.path));
        if !path.exists() {
            eprintln!(
                "note: {} is outside the output directory, so the `{}` package was \
                 not vendored into it. `wasm-tools component embed` resolves a world \
                 from that directory alone, so copy the package to {} before \
                 building, or keep the WIT inside the project.",
                wit_dir.display(),
                componentize_formal::abi_package_name(),
                path.display(),
            );
        }
    } else {
        artifacts.push(abi);
    }

    let wrote = emit::write(out_dir, &artifacts, force)?;

    // One column, so the list reads as a list. The vendored package's path is
    // several segments long and a fixed width would run its description into
    // it.
    let width = wrote
        .iter()
        .map(|w| w.path().display().to_string().chars().count())
        .max()
        .unwrap_or(0);
    for (artifact, result) in artifacts.iter().zip(&wrote) {
        let path = result.path().display().to_string();
        match result {
            emit::Wrote::Written(_) => println!("  {path:<width$}  {}", artifact.what),
            emit::Wrote::Kept(_) => println!("  {path:<width$}  kept, already there"),
        }
    }
    println!(
        "\nWrite {} implementing `Module Type {}`, then `make` in {}.",
        w.cfg.impl_.file.display(),
        plan.module_type,
        normalise(out_dir).display(),
    );
    Ok(ExitCode::SUCCESS)
}

/// A loaded world, with the config that located it.
struct World {
    resolve: Resolve,
    world: WorldId,
    cfg: Config,
}

impl World {
    /// The flags win over the config, which wins over the defaults.
    fn load(input: &Input) -> Result<World> {
        let mut cfg = match &input.config {
            Some(path) => Config::load(path)?,
            None => {
                let wit = input
                    .wit
                    .clone()
                    .context("no WIT given: pass --wit PATH or --config component.toml")?;
                Config::for_wit(wit)
            }
        };
        if let Some(wit) = &input.wit {
            cfg.wit = wit.clone();
        }
        if let Some(world) = &input.world {
            cfg.world = Some(world.clone());
        }
        if let Some(out_dir) = &input.out_dir {
            cfg.out_dir = out_dir.clone();
        }
        if let Some(frontend) = input.frontend {
            cfg.frontend = frontend.into();
        }

        // Paths in a config file are relative to it, and `Config::load` has
        // already joined them. Without one there is no such base, so the
        // implementation file is relative to where the output goes -- which is
        // also the only place the generated build rules can find it.
        if input.config.is_none() {
            cfg.impl_.file = cfg.out_dir.join(&cfg.impl_.file);
        }

        // Before anything else: generating Rocq for someone who asked for Lean
        // would be the exact silent failure this tool exists to avoid.
        cfg.frontend.ensure_supported()?;

        let (resolve, world) = componentize_formal::resolve_world(&cfg.wit, cfg.world.as_deref())?;
        Ok(World {
            resolve,
            world,
            cfg,
        })
    }

    fn gate(&self) -> support::Report {
        support::check_world(&self.resolve, self.world)
    }

    fn name(&self) -> &str {
        &self.resolve.worlds[self.world].name
    }

    /// The directory `wasm-tools component embed` reads.
    ///
    /// `embed` takes a directory, not a file, because it resolves the world's
    /// dependencies from `deps/` beside it. A `--wit world.wit` therefore
    /// builds from the directory that file sits in.
    fn wit_dir(&self) -> Result<PathBuf> {
        let wit = &self.cfg.wit;
        if wit.is_dir() {
            return Ok(wit.clone());
        }
        let parent = wit.parent().filter(|p| !p.as_os_str().is_empty());
        Ok(parent.unwrap_or(Path::new(".")).to_path_buf())
    }
}

/// `path` as the generated build rules should name it: relative to the
/// directory they will be run from.
///
/// Both ends are resolved first, so this works for a WIT directory that is not
/// under the output directory -- `--wit` pointing at a tree kept elsewhere is
/// reasonable, and `make` is perfectly happy with `../`.
fn relative(path: &Path, base: &Path) -> Result<String> {
    let path = path
        .canonicalize()
        .with_context(|| format!("resolving {}", path.display()))?;
    let base = base
        .canonicalize()
        .with_context(|| format!("resolving {}", base.display()))?;

    let shared = path
        .components()
        .zip(base.components())
        .take_while(|(a, b)| a == b)
        .count();
    let up = base.components().skip(shared).count();
    let mut rest: PathBuf = (0..up).map(|_| "..").collect();
    rest.extend(path.components().skip(shared));

    let rest = rest.to_str().context("a path that is not UTF-8")?;
    Ok(match rest.is_empty() {
        true => ".".to_string(),
        false => rest.to_string(),
    })
}

fn same_dir(a: &Path, b: &Path) -> bool {
    normalise(a) == normalise(b)
}

/// Drop the `.` components a joined relative path collects, so two spellings
/// of the same directory compare equal.
fn normalise(path: &Path) -> PathBuf {
    let kept: PathBuf = path
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    match kept.as_os_str().is_empty() {
        true => PathBuf::from("."),
        false => kept,
    }
}
