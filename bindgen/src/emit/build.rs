//! `Makefile`: the rules that turn the two Rocq files and the adapter into a
//! component.
//!
//! Four steps, and the order is the whole content of this file: `rocq c`
//! produces the backend's module, `wasm-merge` links the adapter against it,
//! `wasm-tools component embed` puts the world's type into the result and
//! `component new` wraps it as a component.

use std::fmt::Write;

use super::plan::Plan;
use super::{comment, BACKEND_MODULE, MARKER};
use crate::repr::exports;

/// The name `wasm-merge` is given for the adapter's half of the link. The
/// backend's half is [`BACKEND_MODULE`], which is also the module name every
/// import in the adapter is written against -- the two have to agree, so they
/// are one constant each rather than a string in two places.
const ADAPTER_MODULE: &str = "adapter";

pub fn makefile(plan: &Plan) -> String {
    let b = &plan.build;
    let mut out = String::new();

    out.push_str(&comment::make(&[
        format!(
            "{MARKER} wrote this from the world `{}` and",
            plan.world_name
        ),
        "component.toml.".into(),
        String::new(),
        "`make` assembles the component; `make run ARGS=...` invokes the export.".into(),
        String::new(),
        "Everything here is derived from the world and the config -- the file".into(),
        "names, the export name -- bar one assumption worth naming: that the".into(),
        "module CertiRocq emits exports its function table, which is what the".into(),
        "adapter links the compiled closures through. The first rule below".into(),
        "checks it rather than letting the link fail (TODO.md#table-export).".into(),
    ]));

    writeln!(
        out,
        "\nWASM_NAME  := {}\n\
         COMPONENT  := $(WASM_NAME).comp.wasm\n\
         WIT        := {}\n\
         WIT_FILES  := $(wildcard $(WIT)/*.wit) $(wildcard $(WIT)/deps/*/*.wit)\n\
         EXPORT     := {}\n\
         # Reference types: the adapter imports the backend's function table.\n\
         MERGE_OPTS := --enable-reference-types\n\
         # `make run ARGS=8` invokes the export with 8.\n\
         ARGS       ?=",
        b.wasm_name, b.wit_dir, plan.export.wit_name,
    )
    .unwrap();

    out.push_str(
        "\n.PHONY: all run wit clean\nall: $(COMPONENT)\n\
         \nrun: $(COMPONENT)\n\
         \twasmtime run --invoke '$(EXPORT)($(ARGS))' $<\n\
         \n# The component's own view of itself, which is the world it was embedded with.\n\
         wit: $(COMPONENT)\n\
         \twasm-tools component wit $<\n",
    );

    writeln!(
        out,
        "\n# ---------------------------------------------------------------------------\n\
         # The Rocq half.\n\
         %.vo: %.v\n\
         \trocq c $<\n\
         \n# {impl_}.v implements the generated Module Type, so {interface}.vo comes first.\n\
         {impl_}.vo: {interface}.vo\n\
         \n$(WASM_NAME).wasm: {extraction}.v {impl_}.vo\n\
         \trocq c {extraction}.v",
        impl_ = b.impl_stem,
        interface = b.interface_stem,
        extraction = b.extraction_stem,
    )
    .unwrap();

    writeln!(
        out,
        "\n# ---------------------------------------------------------------------------\n\
         # The component half.\n\
         %.wasm: %.wat\n\
         \twasm-tools parse $< -o $@\n\
         \n# The adapter calls the compiled closures through the backend's table, so the\n\
         # export has to be there. A CertiRocq predating it fails here, with the reason,\n\
         # rather than at link time with a message about a missing symbol.\n\
         $(COMPONENT): $(WASM_NAME).wasm adapter.wasm $(WIT_FILES)\n\
         \t@wasm-tools parse $(WASM_NAME).wasm -t \\\n\
         \t  | grep -q '(export \"{table}\"' \\\n\
         \t  || {{ echo 'error: this CertiRocq does not export its function table'; \\\n\
         \t       echo '       as \"{table}\", which the adapter links the compiled'; \\\n\
         \t       echo '       closures through. Move the CertiRocq you build against'; \\\n\
         \t       echo '       forward -- see TODO.md#table-export.'; \\\n\
         \t       exit 1; }}\n\
         \twasm-merge $(WASM_NAME).wasm {backend} adapter.wasm {adapter} $(MERGE_OPTS) -o merged.wasm\n\
         \twasm-tools component embed $(WIT) merged.wasm -o embedded.wasm\n\
         \twasm-tools component new embedded.wasm -o $@",
        table = exports::TABLE,
        backend = BACKEND_MODULE,
        adapter = ADAPTER_MODULE,
    )
    .unwrap();

    writeln!(
        out,
        "\nclean:\n\
         \trm -f *.wasm *.vo *.vok *.vos *.glob .*.aux *.ir"
    )
    .unwrap();
    out
}
