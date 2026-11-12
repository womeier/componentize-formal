//! `Makefile`: the rules that turn the two Rocq files and the adapter into a
//! component.
//!
//! Three steps, and the order is the whole content of this file: `rocq c`
//! produces the backend's module, `sed` splices it and the adapter into
//! `component.wat` as the two core modules, and `wasm-tools parse` assembles
//! the result. There is no link step rewriting either module -- the
//! component resolves the adapter's imports by instantiation.

use std::fmt::Write;

use super::plan::Plan;
use super::{comment, ADAPTER_MODULE, BACKEND_MODULE, MARKER};
use crate::repr::exports;

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
        "checks it rather than letting instantiation fail (TODO.md#table-export).".into(),
    ]));

    writeln!(
        out,
        "\nWASM_NAME  := {}\n\
         COMPONENT  := $(WASM_NAME).comp.wasm\n\
         EXPORT     := {}\n\
         # `make run ARGS=8` invokes the export with 8.\n\
         ARGS       ?=",
        b.wasm_name, plan.export.wit_name,
    )
    .unwrap();

    out.push_str(
        "\n.PHONY: all run wit clean\nall: $(COMPONENT)\n\
         \nrun: $(COMPONENT)\n\
         \twasmtime run --invoke '$(EXPORT)($(ARGS))' $<\n\
         \n# The component's own view of itself, which is the world it was built with.\n\
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
         # The component half. component.wat already holds the world's type; what is\n\
         # missing is its two core modules. `wasm-tools print` round-trips the backend\n\
         # through text so `sed` can splice it in -- nothing rewrites either module.\n\
         \n# The adapter calls the compiled closures through the backend's table, so the\n\
         # export has to be there. A CertiRocq predating it fails here, with the reason,\n\
         # rather than at instantiation time with a message about a missing export.\n\
         $(COMPONENT): $(WASM_NAME).wasm adapter.wat component.wat\n\
         \t@wasm-tools parse $(WASM_NAME).wasm -t \\\n\
         \t  | grep -q '(export \"{table}\"' \\\n\
         \t  || {{ echo 'error: this CertiRocq does not export its function table'; \\\n\
         \t       echo '       as \"{table}\", which the adapter links the compiled'; \\\n\
         \t       echo '       closures through. Move the CertiRocq you build against'; \\\n\
         \t       echo '       forward -- see TODO.md#table-export.'; \\\n\
         \t       exit 1; }}\n\
         \twasm-tools print $(WASM_NAME).wasm \\\n\
         \t  | sed '1s|^(module|(core module $${backend}|' > linked-{backend}.wat\n\
         \tsed -n '/^(module/,$$p' adapter.wat \\\n\
         \t  | sed '1s|^(module|(core module $${adapter}|' > linked-{adapter}.wat\n\
         \tsed -e '/@BACKEND@/r linked-{backend}.wat' -e '/@BACKEND@/d' \\\n\
         \t    -e '/@ADAPTER@/r linked-{adapter}.wat' -e '/@ADAPTER@/d' \\\n\
         \t    component.wat > linked.wat\n\
         \twasm-tools parse linked.wat -o $@",
        table = exports::TABLE,
        backend = BACKEND_MODULE,
        adapter = ADAPTER_MODULE,
    )
    .unwrap();

    writeln!(
        out,
        "\nclean:\n\
         \trm -f *.wasm linked*.wat *.vo *.vok *.vos *.glob .*.aux *.ir"
    )
    .unwrap();
    out
}
