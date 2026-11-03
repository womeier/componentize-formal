//! (Fields consumed by the generator land in phase 1; the config shape is
//! settled now so examples can be written against it.)
#![allow(dead_code)]

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
    /// Required only when the package declares more than one world.
    #[serde(default)]
    pub world: Option<String>,
    #[serde(default = "default_out_dir")]
    pub out_dir: PathBuf,
    #[serde(default, rename = "impl")]
    pub impl_: ImplConfig,
    #[serde(default)]
    pub wasm: WasmConfig,
    #[serde(default)]
    pub certirocq: CertiRocqConfig,
    #[serde(default)]
    pub abi: AbiConfig,
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

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct AbiConfig {
    #[serde(default)]
    pub oom: Oom,
}

/// How the adapter reports a failure.
#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Oom {
    /// Through the error channel, which every export is then required to
    /// have: `result<T, certirocq:abi/errors.error>`.
    #[default]
    Report,
    /// Trap instead, which lets an export return a bare type at the cost of
    /// taking the whole instance down on a failure.
    Trap,
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
    PathBuf::from("gen")
}

impl Config {
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
