# Assembles the component two ways: against the module CertiRocq emits, and
# against the stub that stands in for it. These are the build rules
# `componentize-formal generate` has to emit; nothing here is hand-tuned to
# minus beyond the file names.

WASM_NAME  := rocq-minus
# `a,b` pairs, split in the recipes. Two arguments is the point of this
# example, so every case exercises both.
INVOKE     := 10,3 3,10 0,1 9223372036854775807,9223372036854775807 \
              9223372036854775808,0 0,9223372036854775808
MERGE_OPTS := --enable-reference-types

.PHONY: all run run-stub run-stub-oom clean
all: run-stub

# ---------------------------------------------------------------------------
# The real path: CertiRocq's output, linked against the adapter.
run: $(WASM_NAME).comp.wasm
	wasm-tools component wit $<
	@for v in $(INVOKE); do \
	  a=$${v%,*}; b=$${v#*,}; \
	  printf 'minus(%s, %s) = ' "$$a" "$$b"; \
	  wasmtime run --invoke "minus($$a, $$b)" $< ; \
	done

%.vo: %.v
	rocq c $<

# impl.v implements the generated Module Type, so interface.vo comes first.
impl.vo: interface.vo

$(WASM_NAME).wasm: extraction.v impl.vo
	rocq c extraction.v

# The adapter links the compiled closures through the table, so the export has
# to be there. A CertiRocq predating it fails here rather than at link time
# with a message about a missing symbol.
$(WASM_NAME).comp.wasm: $(WASM_NAME).wasm adapter.wasm wit/world.wit
	@wasm-tools parse $(WASM_NAME).wasm -t \
	  | grep -q '(export "table"' \
	  || { echo 'error: this CertiRocq does not export its function table as'; \
	       echo '       "table", which the adapter links the compiled closures'; \
	       echo '       through. Move the `certirocq` flake input forward -- see'; \
	       echo '       TODO.md#table-export. `make run-stub` exercises the'; \
	       echo '       adapter without it.'; \
	       exit 1; }
	wasm-merge $(WASM_NAME).wasm rocq adapter.wasm adapter $(MERGE_OPTS) -o merged.wasm
	wasm-tools component embed wit merged.wasm -o embedded.wasm
	wasm-tools component new embedded.wasm -o $@

# ---------------------------------------------------------------------------
# The stub path: no Rocq, no pin, runs today. Exercises the adapter's half --
# the argument loop, the lowering, the discriminants, and all three checks.
run-stub: stub.comp.wasm
	@for v in $(INVOKE); do \
	  a=$${v%,*}; b=$${v#*,}; \
	  printf 'minus(%s, %s) = ' "$$a" "$$b"; \
	  wasmtime run --invoke "minus($$a, $$b)" $< ; \
	done

run-stub-oom: stub-oom.comp.wasm
	@printf 'minus(10, 3) with the heap exhausted between the applications = '
	@wasmtime run --invoke 'minus(10, 3)' $<

stub.comp.wasm: stub/backend.wasm adapter.wasm wit/world.wit
	wasm-merge stub/backend.wasm rocq adapter.wasm adapter $(MERGE_OPTS) -o stub-merged.wasm
	wasm-tools component embed wit stub-merged.wasm -o stub-embedded.wasm
	wasm-tools component new stub-embedded.wasm -o $@

stub-oom.comp.wasm: stub/oom.wasm adapter.wasm wit/world.wit
	wasm-merge stub/oom.wasm rocq adapter.wasm adapter $(MERGE_OPTS) -o stub-oom-merged.wasm
	wasm-tools component embed wit stub-oom-merged.wasm -o stub-oom-embedded.wasm
	wasm-tools component new stub-oom-embedded.wasm -o $@

%.wasm: %.wat
	wasm-tools parse $< -o $@

clean:
	rm -f *.wasm stub/*.wasm *.vo *.vok *.vos *.glob .*.aux *.ir
