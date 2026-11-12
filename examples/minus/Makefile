# Assembles the component two ways: against the module CertiRocq emits, and
# against the stub that stands in for it. These are the build rules
# `componentize-formal generate` has to emit; nothing here is hand-tuned to
# minus beyond the file names.

WASM_NAME  := rocq-minus
# `a,b` pairs, split in the recipes. Two arguments is the point of this
# example, so every case exercises both.
INVOKE     := 10,3 3,10 0,1 9223372036854775807,9223372036854775807 \
              9223372036854775808,0 0,9223372036854775808

# Splice the two core modules into the component shell and assemble it.
# $(1) is the backend module, $(2) a prefix for the intermediates, $(3) the
# component to write. Both modules go in exactly as they were produced --
# `wasm-tools print` round-trips them through text, nothing rewrites them.
define link
	wasm-tools print $(1) | sed '1s|^(module|(core module $$rocq|' > $(2)-rocq.wat
	sed -n '/^(module/,$$p' adapter.wat | sed '1s|^(module|(core module $$adapter|' > $(2)-adapter.wat
	sed -e '/@BACKEND@/r $(2)-rocq.wat' -e '/@BACKEND@/d' \
	    -e '/@ADAPTER@/r $(2)-adapter.wat' -e '/@ADAPTER@/d' component.wat > $(2)-linked.wat
	wasm-tools parse $(2)-linked.wat -o $(3)
endef

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
# to be there. A CertiRocq predating it fails here rather than at
# instantiation time with a message about a missing export.
$(WASM_NAME).comp.wasm: $(WASM_NAME).wasm adapter.wat component.wat
	@wasm-tools parse $(WASM_NAME).wasm -t \
	  | grep -q '(export "table"' \
	  || { echo 'error: this CertiRocq does not export its function table as'; \
	       echo '       "table", which the adapter links the compiled closures'; \
	       echo '       through. Move the `certirocq` flake input forward -- see'; \
	       echo '       TODO.md#table-export. `make run-stub` exercises the'; \
	       echo '       adapter without it.'; \
	       exit 1; }
	$(call link,$(WASM_NAME).wasm,real,$@)

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

stub.comp.wasm: stub/backend.wasm adapter.wat component.wat
	$(call link,stub/backend.wasm,stub,$@)

stub-oom.comp.wasm: stub/oom.wasm adapter.wat component.wat
	$(call link,stub/oom.wasm,stub-oom,$@)

%.wasm: %.wat
	wasm-tools parse $< -o $@

clean:
	rm -f *.wasm *-rocq.wat *-adapter.wat *-linked.wat stub/*.wasm *.vo *.vok *.vos *.glob .*.aux *.ir
