# Assembles the component two ways: against the module CertiRocq emits, and
# against the stub that stands in for it. These are the build rules
# `componentize-formal generate` has to emit; nothing here is hand-tuned to
# grow beyond the file names.

WASM_NAME  := rocq-grow
MERGE_OPTS := --enable-reference-types --enable-tail-call

# WAVE spells a record `{field:value}`, with the WIT field names. The braces
# are added by the recipe rather than written here, because `{a,b}` in a shell
# word is brace expansion and the loop would see two arguments where one was
# meant. The last two differ in which field is out of range: each field is its
# own check, so one of them passing is not evidence about the other.
INVOKE := \
  start:10,byte-length:1 \
  start:0,byte-length:0 \
  start:7,byte-length:9223372036854775807 \
  start:9223372036854775808,byte-length:0 \
  start:0,byte-length:9223372036854775808

.PHONY: all run run-stub run-stub-oom clean
all: run-stub

# ---------------------------------------------------------------------------
# The real path: CertiRocq's output, linked against the adapter.
run: $(WASM_NAME).comp.wasm
	wasm-tools component wit $<
	@for v in $(INVOKE); do \
	  printf 'grow({%s}) = ' "$$v"; \
	  wasmtime run --invoke "grow({$$v})" $< ; \
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
# the lowering, the discriminants, and both checks firing.
run-stub: stub.comp.wasm
	@for v in $(INVOKE); do \
	  printf 'grow({%s}) = ' "$$v"; \
	  wasmtime run --invoke "grow({$$v})" $< ; \
	done

run-stub-oom: stub-oom.comp.wasm
	@printf 'grow({start:10,byte-length:1}) with the heap exhausted = '
	@wasmtime run --invoke 'grow({start:10,byte-length:1})' $<

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
