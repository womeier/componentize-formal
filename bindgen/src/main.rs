//! Generate a Rocq interface, a CertiRocq extraction file and canonical-ABI
//! glue from a WIT world.
//!
//! Today only `check` is real: it decides whether a world is within the subset
//! the backend can honour, and points at `TODO.md` for everything it turns
//! away. Generation lands in phase 1.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use componentize_formal::{config, config::Frontend, config::Oom, support};
use wit_parser::Resolve;

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
    /// Write Iface.v, extraction.v, adapter.wat and the build rules.
    Generate(Input),
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
    /// Treat warnings as errors. Warnings mark narrowings the adapter has to
    /// guard at runtime, so a build that wants no surprises should set this.
    #[arg(long)]
    deny_warnings: bool,
    /// How a failure is reported: through the error channel every export is
    /// then required to have, or by trapping. Overrides the config.
    #[arg(long, value_enum)]
    oom: Option<OomArg>,
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

impl From<FrontendArg> for Frontend {
    fn from(arg: FrontendArg) -> Self {
        match arg {
            FrontendArg::Rocq => Frontend::Rocq,
            FrontendArg::Lean => Frontend::Lean,
        }
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum OomArg {
    /// Require result<T, certirocq:abi/errors.error> on every export.
    Report,
    /// Trap on failure, allowing a bare result type.
    Trap,
}

impl From<OomArg> for Oom {
    fn from(arg: OomArg) -> Self {
        match arg {
            OomArg::Report => Oom::Report,
            OomArg::Trap => Oom::Trap,
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
        Cmd::Check(input) => {
            let (resolve, world, cfg) = resolve_world(input)?;
            frontend_of(input, cfg.as_ref()).ensure_supported()?;
            let report = support::check_world(&resolve, world, oom_of(input, cfg.as_ref()));

            for warning in &report.warnings {
                eprintln!("warning: {warning}");
            }
            for rejection in &report.rejections {
                eprintln!("{rejection}");
            }

            if report.is_ok() {
                if input.deny_warnings && !report.warnings.is_empty() {
                    eprintln!(
                        "\n{} warning(s) in world `{}`, and --deny-warnings is set",
                        report.warnings.len(),
                        resolve.worlds[world].name
                    );
                    return Ok(ExitCode::FAILURE);
                }
                println!(
                    "ok: world `{}` is within the supported subset",
                    resolve.worlds[world].name
                );
                Ok(ExitCode::SUCCESS)
            } else {
                eprintln!(
                    "\n{} unsupported construct(s) in world `{}`",
                    report.rejections.len(),
                    resolve.worlds[world].name
                );
                Ok(ExitCode::FAILURE)
            }
        }
        Cmd::Generate(input) => {
            let (resolve, world, cfg) = resolve_world(input)?;
            frontend_of(input, cfg.as_ref()).ensure_supported()?;
            let report = support::check_world(&resolve, world, oom_of(input, cfg.as_ref()));
            // Warnings describe narrowings the generated adapter has to guard
            // at runtime, so they are never suppressed on the way to output.
            for warning in &report.warnings {
                eprintln!("warning: {warning}");
            }
            if input.deny_warnings && !report.warnings.is_empty() {
                bail!("warnings present and --deny-warnings is set; nothing generated");
            }
            if !report.is_ok() {
                for rejection in &report.rejections {
                    eprintln!("{rejection}");
                }
                bail!(
                    "world `{}` is outside the supported subset; nothing generated",
                    resolve.worlds[world].name
                );
            }
            bail!("generation is not implemented yet (phase 1); `check` is")
        }
    }
}

/// The flag wins over the config, which wins over the default.
fn frontend_of(input: &Input, cfg: Option<&config::Config>) -> Frontend {
    input
        .frontend
        .map(Frontend::from)
        .unwrap_or_else(|| cfg.map(|c| c.frontend).unwrap_or_default())
}

/// The flag wins over the config, which wins over the default.
fn oom_of(input: &Input, cfg: Option<&config::Config>) -> Oom {
    input
        .oom
        .map(Oom::from)
        .unwrap_or_else(|| cfg.map(|c| c.abi.oom).unwrap_or_default())
}

fn resolve_world(input: &Input) -> Result<(Resolve, wit_parser::WorldId, Option<config::Config>)> {
    let cfg = match &input.config {
        Some(path) => Some(config::Config::load(path)?),
        None => None,
    };

    let wit = input
        .wit
        .clone()
        .or_else(|| cfg.as_ref().map(|c| c.wit.clone()))
        .context("no WIT given: pass --wit PATH or --config component.toml")?;

    let world_name = input
        .world
        .clone()
        .or_else(|| cfg.as_ref().and_then(|c| c.world.clone()));

    let (resolve, world) = componentize_formal::resolve_world(&wit, world_name.as_deref())?;
    Ok((resolve, world, cfg))
}
