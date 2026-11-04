# Assembles the component two ways: against the module CertiRocq emits, and
# against the stub that stands in for it. These are the build rules
# `componentize-formal generate` has to emit; nothing here is hand-tuned to
# add-byte beyond the file names.

WASM_NAME  := rocq-add-byte
MERGE_OPTS := --enable-reference-types

# WAVE spells a variant case `case(payload)`, and bare for a case that carries
# none. The parentheses need quoting, so each call is its own line rather than
# a loop over a list.
answer = printf 'add-byte(%s) = ' '$(1)'; wasmtime run --invoke 'add-byte($(1))' $(2)

.PHONY: all run run-stub run-stub-oom clean
all: run-stub

# ---------------------------------------------------------------------------
# The real path: CertiRocq's output, linked against the adapter.
run: $(WASM_NAME).comp.wasm
	wasm-tools component wit $<
	@$(call answer,unknown,$<)
	@$(call answer,exactly(41),$<)
	@$(call answer,at-least(7),$<)
	@$(call answer,exactly(9223372036854775807),$<)
	@$(call answer,too-big,$<)
	@$(call answer,exactly(9223372036854775808),$<)

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
	@$(call answer,unknown,$<)
	@$(call answer,exactly(41),$<)
	@$(call answer,at-least(7),$<)
	@$(call answer,exactly(9223372036854775807),$<)
	@$(call answer,too-big,$<)
	@$(call answer,exactly(9223372036854775808),$<)

run-stub-oom: stub-oom.comp.wasm
	@printf 'add-byte(exactly(41)) with the heap exhausted = '
	@wasmtime run --invoke 'add-byte(exactly(41))' $<

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
