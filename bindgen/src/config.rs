//! `component.toml`: everything the generator needs that does not come out of
//! the WIT file itself.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    /// The WIT file or directory, relative to this config file.
    pub wit: PathBuf,
    /// The proof assistant the implementation is written in.
    #[serde(default)]
    pub frontend: Frontend,
    /// Required only when the package declares more than one world.
    #[serde(default)]
    pub world: Option<String>,
    /// Where the generated files go. Defaults to the config's own directory,
    /// and has to stay with `impl.v`: the generated `interface.v` is
    /// `Require Import`ed by it and `extraction.v` requires `impl.v` back, so
    /// Rocq wants all three on one load path.
    #[serde(default = "default_out_dir")]
    pub out_dir: PathBuf,
    #[serde(default, rename = "impl")]
    pub impl_: ImplConfig,
    #[serde(default)]
    pub wasm: WasmConfig,
    #[serde(default)]
    pub certirocq: CertiRocqConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ImplConfig {
    /// The file you write, implementing the generated `Module Type`.
    pub file: PathBuf,
    /// The module inside it that CertiRocq should compile.
    pub module: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct WasmConfig {
    /// The `-file` argument, so the backend writes `<name>.wasm`.
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CertiRocqConfig {
    /// Passed through verbatim to `CertiRocq Compile Wasm`.
    #[serde(default)]
    pub flags: Vec<String>,
    /// Also emit `CertiRocq Show IR`, which is handy when an adapter misreads a
    /// closure.
    #[serde(default)]
    pub show_ir: bool,
}

/// Which proof assistant the implementation is written in.
///
/// Both reach this Wasm backend through λ□, so the adapter, the build rules
/// and the support gate are shared; what differs is the interface file and the
/// compile driver. Only Rocq is implemented, and `lean` is rejected rather
/// than quietly treated as `rocq` -- see `TODO.md#frontends`.
#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Frontend {
    #[default]
    Rocq,
    Lean,
}

impl Frontend {
    pub fn name(self) -> &'static str {
        match self {
            Frontend::Rocq => "rocq",
            Frontend::Lean => "lean",
        }
    }

    /// Fail unless this frontend is actually implemented.
    ///
    /// Selecting an unimplemented one has to be an error: generating Rocq
    /// artefacts for someone who asked for Lean would be the exact silent
    /// failure this tool exists to avoid.
    pub fn ensure_supported(self) -> Result<()> {
        match self {
            Frontend::Rocq => Ok(()),
            Frontend::Lean => anyhow::bail!(
                "the `lean` frontend is not implemented yet\n  = {}\n  = {}",
                "only `rocq` is. Lean needs its own interface file and compile \
                 driver, and three representation questions settled first: how \
                 `UInt64` erases, what order erasure gives constructors, and \
                 what `peregrine wasm` emits",
                "see TODO.md#frontends",
            ),
        }
    }
}

impl Default for ImplConfig {
    fn default() -> Self {
        Self {
            file: PathBuf::from("impl.v"),
            module: "Impl".into(),
        }
    }
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            name: "rocq-world".into(),
        }
    }
}

fn default_out_dir() -> PathBuf {
    PathBuf::from(".")
}

impl Config {
    /// The config a bare `--wit PATH` implies: every default, and that path.
    ///
    /// `generate` needs more than the WIT file says -- a `-file` name, which
    /// module to compile -- so working without a `component.toml` means
    /// working against its defaults rather than against nothing.
    pub fn for_wit(wit: PathBuf) -> Self {
        let out_dir = default_out_dir();
        Config {
            wit,
            frontend: Frontend::default(),
            world: None,
            out_dir,
            impl_: ImplConfig::default(),
            wasm: WasmConfig::default(),
            certirocq: CertiRocqConfig::default(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;

        // Paths in the file read naturally only if they are relative to it.
        if let Some(base) = path.parent().filter(|b| !b.as_os_str().is_empty()) {
            cfg.wit = base.join(&cfg.wit);
            cfg.out_dir = base.join(&cfg.out_dir);
            cfg.impl_.file = base.join(&cfg.impl_.file);
        }
        Ok(cfg)
    }
}
