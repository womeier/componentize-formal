# Deferred work

What `rocq-component-bindgen` deliberately does not do, and what each item would
cost. Nothing here needs to work for the tool to be useful; the point is that a
WIT world using any of it is **rejected with a clear message** instead of
producing an adapter that misreads memory.

The gate lives in [`bindgen/src/support.rs`](bindgen/src/support.rs)
(`check_world`), and every rejection prints the code and the anchor of its
section below:

```
$ rocq-component-bindgen check --wit world.wit
error[E-LIST]: list (list<u32>) is not supported
  --> export total: param `xs`
  = out of scope: needs cabi_realloc and cons-cell walking in both directions, ...
  = see TODO.md#lists
```

Fixtures for every code are in [`bindgen/tests/wit`](bindgen/tests/wit) and
pinned by [`bindgen/tests/support.rs`](bindgen/tests/support.rs). That test also
asserts each code is mentioned here, so a new code cannot be added without a
section.

| Code | Construct | Status |
| --- | --- | --- |
| `E-ARITY-0`, `E-ARITY-N`, `E-NO-RESULT` | exports that aren't one-in-one-out | [planned, phase 2](#arity) |
| `E-MULTI-EXPORT` | more than one exported function | [planned, phase 2](#multi-export) |
| `E-STRUCTURED` | `record`, `tuple`, `variant`, nested `option`/`result` | [planned, phase 3](#structured-types) |
| `E-SIGNED` | `s8`, `s16`, `s32`, `s64` | [wanted, not scheduled](#signed-integers) |
| `E-FLOAT` | `f64` | [planned, phase 4](#f64) |
| `E-FLOAT` | `f32` | [wanted, not scheduled](#floats) |
| `E-LIST`, `E-MAP` | `list`, fixed-length list, `map` | [out of scope](#lists) |
| `E-STRING` | `string` | [out of scope](#strings) |
| `E-CHAR` | `char` | [out of scope](#char) |
| `E-FLAGS` | `flags` | [out of scope](#flags) |
| `E-RESOURCE` | `resource`, `own`, `borrow`, methods | [out of scope](#resources) |
| `E-ASYNC` | `async func`, `future`, `stream`, `error-context` | [out of scope](#async) |
| `E-IMPORT`, `E-NO-EXPORT`, `E-UNKNOWN` | imported functions and interfaces | [out of scope](#imports) |
| `E-EXPORT-INTERFACE` | `export some:pkg/iface` | [out of scope](#exported-interfaces) |
| — | verified glue | [research project](#verified-glue) |

## No silent failure

A wrong adapter does not announce itself: it reads the wrong bytes and
`wasmtime` hands back plausible garbage. So the gate is deliberately one-sided.

- Every construct is matched explicitly. Nothing is accepted merely because the
  walk failed to recognise it — an unresolved type is itself a rejection
  (`E-UNKNOWN`).
- Every offending position is reported, not just the first, so one run tells you
  the whole story.
- Anything that *narrows* rather than rejects produces a warning naming the
  position: `u64` carried by a 63-bit integer, or a result type with nowhere to
  put an out-of-memory signal (where the adapter will trap). `--deny-warnings`
  turns those into failures.
- A construct that cannot be represented is rejected even when it is only
  declared and never used by an export, because it still shows up in the
  component's type. A world-level `resource` is the case that matters.
- Out-of-memory is never swallowed. `glob_out_of_mem` is checked after every
  call, and reported through `none`/`err` where the result type allows it and a
  trap where it does not.

---

## Planned

Integers are carried by **Rocq's primitive 63-bit unsigned ints** (`Uint63.int`)
and nothing else. `u8`, `u16` and `u32` fit exactly. `u64` does not: values
≥ 2⁶³ have no representation, so the generator warns at the position where the
narrowing happens and the adapter guards rather than truncating.

### <a id="arity"></a>Export shape — `E-ARITY-0`, `E-ARITY-N`, `E-NO-RESULT`, phase 2

Only `func(x: T) -> U` is handled, matching
`testing/examples/component/plus1`. The three other shapes each need their own
adapter prologue:

- **No parameters.** A nullary export is a value, not a closure, so
  `main_function` leaves the result itself in `glob_result` and the adapter
  skips `call_indirect` entirely.
- **Several parameters.** Needs iterated closure application: one
  `call_indirect` per argument, reading `glob_result` between calls to get the
  next closure. This is what the C glue's `make_call`
  (`theories/Glue/glue.v:1229`) does, one argument at a time.
  **Open risk:** whether uncurrying changes the arity of the closure reachable
  from `main`'s result. Verify with a real two-argument example before trusting
  one-argument-at-a-time.
- **No result.** Nothing to lift, and nowhere to put the out-of-memory signal.

### <a id="multi-export"></a>Several exports — `E-MULTI-EXPORT`, phase 2

`CertiRocq Compile Wasm` compiles one closed term, and `main_function` leaves
one value in `glob_result`. For a world with *n* exports the generated
`extraction.v` will compile a single *n*-ary constructor holding every export
closure:

```coq
Inductive Exports := MkExports (plus1 : int -> int) (negb : bool -> bool).
Definition __exports := MkExports Impl.plus1 Impl.negb.
```

A single-constructor inductive of arity *n* is boxed as `[ord, f₀, …, fₙ₋₁]`
(`LambdaANF_to_Wasm.v:161`), so export *i*'s adapter loads `offset=4*(i+1)` to
get its closure and proceeds as the one-export case does. A Rocq tuple would
*not* work: `(f, g, h)` is right-nested `pair f (pair g h)`, so it needs a spine
walk instead of a flat index. `n = 1` skips the wrapper, keeping the generated
adapter comparable to the hand-written `plus1` one.

### <a id="structured-types"></a>Records, tuples and variants — `E-STRUCTURED`, phase 3

`record`, `tuple`, `variant`, and any `option`/`result` that isn't the top-level
result type. Two halves:

1. **Our side.** CertiRocq assigns constructor ordinals *separately per class* —
   `match_ordinals_with_tag` (`theories/Glue/glue.v:947-960`) walks constructors
   keeping independent boxed and unboxed counters. WIT's single case index
   therefore needs a per-case translation table, not an offset. That function is
   the thing to port.
2. **Their side.** Canonical-ABI record and variant layout: discriminant size,
   alignment as the max of field alignments, payload offset, total size. This
   must match `wasm-tools` byte for byte or `wasmtime` silently reads garbage —
   which is exactly why the tool computes it with `wit_parser::SizeAlign`, the
   same code `wasm-tools component new` uses, instead of reimplementing it.

A top-level `option<T>`/`result<T, E>` result is already accepted, because
that is the out-of-memory channel rather than a structured type.

### <a id="f64"></a>`f64` — `E-FLOAT`, phase 4

Waiting on the backend's `wasm-prim-floats` branch, where an `f64` is boxed just
as a primitive int is — a pointer to 8 bytes holding the value — so the lift and
lower are the `int63` shape with `f64.load`/`f64.store` in place of the i64
ones. `main` still rejects `primFloat` in `translate_primitive_value`
(`theories/CodegenWasm/LambdaANF_to_Wasm.v:238`), so this unblocks when that
branch lands.

One thing to confirm when implementing: NaN canonicalisation is `canon lift`'s
job in the canonical ABI, which would mean the adapter only has to move the
bits. Worth checking against `wasm-tools` rather than assuming.

---

## Wanted, not scheduled

### <a id="floats"></a>`f32` — `E-FLOAT`

Rocq's `PrimFloat` is binary64 only, so `f32` needs an explicit rounding
convention in both directions before it can be mapped. `f64` is
[phase 4](#f64).

### <a id="signed-integers"></a>Signed integers — `E-SIGNED`

Only unsigned integers are supported. `s8` … `s64` need a decision first:
`Sint63.int` (clean semantics, but `s64` does not fit in 63 bits) or
`Uint63.int` under a two's-complement reading (fits the bit patterns, but then
the Rocq side computes arithmetic that isn't the WIT arithmetic).

`s8` … `s32` are easy either way. `s64` has the same problem `u64` already has,
one bit short.

---

## Out of scope

### <a id="lists"></a>Lists and maps — `E-LIST`, `E-MAP`

`list<T>`, fixed-length lists, and `map<K, V>`. This is the one part that needs
real canonical-ABI machinery:

- `cabi_realloc`, because a caller lowering a list has to allocate in *our*
  memory. Against a bump allocator this is nearly trivial — allocate and copy,
  never free — but it has to exist, along with the ownership and `post-return`
  questions that come with it.
- Cons-cell walking in both directions: building a Rocq `list` from a flat
  canonical-ABI buffer, and walking one back out.

Deliberately cut. It was the largest and riskiest chunk of the original
estimate, and dropping it is what makes the rest self-contained rather than
phased. `map` additionally needs a key ordering both sides agree on.

### <a id="strings"></a>Strings — `E-STRING`

A `string` is a list of bytes plus an encoding choice that `canon lift` forces
on you, so it needs everything [lists](#lists) need and a decision about
`string` vs `list ascii` vs a byte array on the Rocq side.

### <a id="char"></a>`char` — `E-CHAR`

WIT's `char` is a Unicode scalar value. Rocq's `ascii` is 8-bit and `Stdlib` has
nothing matching it, so there is no faithful mapping to pick.

### <a id="flags"></a>`flags` — `E-FLAGS`

A bitfield with no natural Rocq counterpart. A record of booleans is a different
WIT type with a different ABI, so mapping `flags` to one would be a lie about
the interface.

### <a id="resources"></a>Resources and handles — `E-RESOURCE`

`resource` declarations, `own<T>`, `borrow<T>`, and resource methods (including
constructors and statics). Resources need a handle table and destructors, and
there is no GC to hang them off — the backend's bump allocator never frees. This
is the escape hatch that *would* make recursive user inductives expressible
(an opaque handle plus accessor methods, which is `glue.v`'s accessors lifted to
the component level), so it's the natural thing to revisit if recursive types
become the blocker.

### <a id="async"></a>Async — `E-ASYNC`

`async func`, `future<T>`, `stream<T>`, `error-context`. The async ABI is still
moving, and the generated module is single-threaded and synchronous. Pin the
sync subset.

### <a id="imports"></a>Imports — `E-IMPORT`, `E-NO-EXPORT`, `E-UNKNOWN`

Imported functions and interfaces. `mod_imports := []` is baked into the
correctness proof: `module_instantiate_INV_and_more_hold`
(`LambdaANF_to_Wasm_instantiation.v:1234`) instantiates with `[]`, and the store
invariants assume the resulting index layout. Exports-only keeps the existing
proof intact; a world with imports means reworking `instantiation.v`.

Imported *types*, which is what a `use` statement produces, are fine — they are
just names, and the gate lets them through.

`E-UNKNOWN` is the related failure where a world didn't resolve, usually a
missing dependency package.

### <a id="exported-interfaces"></a>Exported interfaces — `E-EXPORT-INTERFACE`

Only top-level `export name: func(..)` is handled. Exporting a named interface
is mostly a naming question — the adapter would export
`ns:pkg/iface#func` — but it interacts with [several exports](#multi-export),
so it waits for that.

### <a id="verified-glue"></a>Verified glue

The adapter is **trusted**, exactly as the C backend's glue is
(`theories/Glue/glue.v` is unverified). The verified core stays verified and the
component shell is trusted glue — the same trust story CertiRocq already ships
for C.

Making it verified is a research project, not a task:

1. There is no component-model or canonical-ABI formalisation in WasmCert-Rocq,
   so the substrate would have to be built, not extended: component value types
   with their size/alignment/flatten functions, the sync canonical ABI
   (lift/lower/store/load, realloc, trap conditions), and component-level
   instantiation.
2. Verified means no `wasm-merge` — the adapter would have to be generated into
   `mod_funcs`/`mod_exports` inside `LambdaANF_to_Wasm.v` so the whole artifact
   is one module term in the theorem. That reverses this tool's separate-adapter
   design.
3. The theorem worth proving — invoking the export on `⟦a⟧` either signals OOM
   or returns `⟦f a⟧` — needs `⟦·⟧` related through erasure, a
   repeated-call invariant the one-shot `toplevel_theorem.v` doesn't give, and a
   memory-layout induction per type former against the `INV_*` invariants.

Roughly a year of focused work for a Tier 0/1 subset. The cheaper standalone
contribution hiding in here is the **canonical ABI as an executable Rocq spec**,
differential-tested against `wasm-tools` — useful to anyone checking
`wit-bindgen`, and it validates the highest-risk code in this tool without the
lift/lower proofs.
