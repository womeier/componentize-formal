# Deferred work

What `componentize-formal` deliberately does not do, and what each item would
cost. Nothing here needs to work for the tool to be useful; the point is that a
WIT world using any of it is **rejected with a clear message** instead of
producing an adapter that misreads memory.

The gate lives in [`bindgen/src/support.rs`](bindgen/src/support.rs)
(`check_world`), and every rejection prints the code and the anchor of its
section below:

```
$ componentize-formal check --wit world.wit
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
| `E-OPTION` | `option`/`result` as a parameter or nested | [planned, phase 2](#option-result) |
| `E-STRUCTURED` | `record`, `tuple`, `variant` | [planned, phase 3](#structured-types) |
| `E-INT-WIDTH` | `u8`, `u16`, `u32` | [wanted, not scheduled](#integer-widths) |
| `E-SIGNED` | `s8`, `s16`, `s32`, `s64` | [wanted, not scheduled](#signed-integers) |
| `E-FLOAT` | `f64` | [planned, phase 4](#f64) |
| `E-FLOAT` | `f32` | [not planned](#f32) |
| `E-LIST` | `list`, fixed-length list | [wanted, not scheduled](#lists) |
| `E-MAP` | `map` | [not planned](#maps) |
| `E-STRING` | `string` | [wanted, not scheduled](#strings) |
| `E-CHAR` | `char` | [out of scope](#char) |
| `E-FLAGS` | `flags` | [out of scope](#flags) |
| `E-RESOURCE` | `resource`, `own`, `borrow`, methods | [out of scope](#resources) |
| `E-ASYNC` | `async func`, `future`, `stream`, `error-context` | [out of scope](#async) |
| `E-IMPORT` | imported functions and interfaces | [wanted, not scheduled](#imports) |
| `E-NO-CHANNEL`, `E-ERROR-TYPE` | an export that cannot report its failures | [required](#failure-channel) |
| `E-NO-EXPORT` | a world with nothing to export | [rejected outright](#nothing-to-compile) |
| `E-UNKNOWN` | a world that did not resolve | [rejected outright](#unresolved) |
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
- Anything that *narrows* rather than rejects gets a warning naming the
  position, and `--deny-warnings` turns those into failures. Nothing warns
  today: the one narrowing, `u64` carried by a 63-bit integer, is reported as a
  failure mode through the error channel instead, which is strictly louder.
- A construct that cannot be represented is rejected even when it is only
  declared and never used by an export, because it still shows up in the
  component's type. A world-level `resource` is the case that matters.
- Out-of-memory is never swallowed, and neither is anything else: every export
  must be able to report every way it can fail. See
  [the failure channel](#failure-channel).

### <a id="cofix"></a>One silent failure the gate cannot see: cofixpoints

The gate reads the WIT world, so it polices the interface, not the Rocq
implementation — and CertiRocq does not support `tCoFix`, [turning it into a
placeholder rather than rejecting
it](https://github.com/peregrine-project/peregrine-tool/issues/92). A
`CoFixpoint` compiles and returns a wrong answer. WIT has no coinductive types,
so no world can ask for one; the exposure is an implementation using one
internally, which a `Module Type` will admit. Detection looks cheap: `CertiRocq
Show IR` already dumps the IR, and the generator will be driving that
invocation anyway.

## <a id="failure-channel"></a>The failure channel — `E-NO-CHANNEL`, `E-ERROR-TYPE`

Every export must return `result<T, error>` with the `error` enum from the
[`certirocq:abi`](bindgen/wit/certirocq-abi.wit) package this repo ships:

```wit
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export plus1: func(val: u64) -> result<u64, error>;
}
```

**Every call can fail**, so a bare return type is an incomplete contract rather
than a terse one. `main_function` runs the whole program and the adapter
allocates the area it returns through, which is why the backend checks
`glob_out_of_mem` after every call it makes (`LambdaANF_to_Wasm.v:373-382`) and
the adapter has to check it after the one it makes.

`option<T>` is **not** enough, and `plus1` is the example: it can fail two
ways, and `none` cannot say which.

| Failure | Detected |
| --- | --- |
| `out-of-memory` | after the call, from `glob_out_of_mem` |
| `out-of-range` | *before* the call — a `u64` argument of 2^63 or more has no `Uint63.int` |

The gate computes the failure modes a signature can produce and names them in
the rejection, so the shortfall is visible rather than inferred.

The error type is matched **by package, version, interface and type name**, not
by its cases, so a look-alike enum of your own is rejected (`E-ERROR-TYPE`)
instead of being populated with cases that happen to line up — see
[versioning](#abi-package-version).

**There is no opt-out.** A trapping export takes the whole instance down and
tells its caller nothing it can act on, which is not a contract worth
generating; and a `--oom=trap` mode would have been a second adapter shape to
write, test and keep conforming for no gain.

The enum never appears on the Rocq side. `func(x: u64) -> result<u64, error>`
asks you for `plus1 : int -> int` — a Rocq function cannot run out of memory in
its own type, and nothing in Rocq knows a value has to fit a WIT width.

### <a id="abi-package-version"></a>The shipped package is versioned

`package certirocq:abi@0.1.0`, and a world has to spell the version:
`use certirocq:abi/errors@0.1.0.{error}`.

The version is part of the type's identity, not decoration. An `enum`'s case
order *is* its discriminant in the canonical ABI, so appending a case to `error`
changes what the adapter's `i32` means, and a world written against a different
case list would line up silently. So the gate matches package, **version**,
interface and type name — and the shipped version is read off
[`certirocq-abi.wit`](bindgen/wit/certirocq-abi.wit) rather than restated in
Rust, so the two cannot drift.

Only one version is ever in play: the generator pushes its own copy into the
resolve, so a world asking for another fails to resolve, and the error names the
version that is shipped.

When the enum does change: adding a case breaks anyone reading the old one, so
while this is `0.x` it takes the minor (semver's breaking slot below 1.0), and
the major afterwards. The version exists to announce the change, not to promise
compatibility across it.

### <a id="refinement-types"></a>Idea: refinement types for the narrow widths

Worth exploring, not scheduled. The reason `u8`/`u16`/`u32` are
[rejected](#integer-widths) is that lifting needs a range check, because nothing
stops the Rocq side returning a larger `int`. A dependent type moves that
obligation to compile time:

```coq
Definition u32 := { x : int | (x <? 2 ^ 32)%uint63 = true }.
Parameter narrow : int -> u32.   (* generated into Iface.v *)
```

Then there is no `out-of-range` on the result side at all — it is ruled out
statically, and the implementer discharges it once instead of the adapter
checking it on every call.

What makes it work at the Wasm level is that the proof is erased: `exist x pf`
becomes a boxed constructor whose proof slot is a box, so the **adapter can
fabricate it** after doing its own check on the way *in*. Nothing has to
construct a proof at runtime.

Three things to settle before taking it seriously:

1. **The erased arity.** `sig` has two fields and the second is a `Prop`, so
   the adapter has to know whether the erased constructor is `[ord, x, box]` or
   whether a pass strips the `Prop` field to `[ord, x]`. This is the
   "erased arity is not the Rocq arity" problem in its sharpest form, and it
   must be checked against `CertiRocq Show IR` rather than assumed — a good
   first case for the [representation suite](#representation).
2. **The trust boundary moves.** On the way in, the adapter asserts the
   refinement by writing a box where the proof goes. A bug in its range check
   then hands Rocq a value violating Rocq's own invariant. That is not worse in
   kind than the rest of the trusted glue, but it is worth naming.
3. **Ergonomics.** Every implementation owes a proof per result. Fine for
   `fun n => n mod 2 ^ 32`; a real burden in general. It would want to be
   opt-in — `abi.narrow = "checked" | "refined"` — rather than the default.

---

## <a id="representation"></a>Representation conformance

The adapter bakes CertiRocq's value representation into loads, stores and
offsets, and nothing checks that on either side. Three layers.

**1a — the formulas.** Every constant lives in
[`repr.rs`](bindgen/src/repr.rs) with its source, and the unit tests pin them
against the hand-written adapters in `testing/examples/component/{plus1,negb}`.
Catches a mistake in `repr.rs`, not the backend moving underneath it.

**1b — the module's shape.** `checks.representation` does this now:
[`checks/representation.py`](checks/representation.py) compiles
[`exports.v`](checks/rocq/exports.v) with the real CertiRocq and reads back
everything visible without running the module — the five export names present,
the table absent (why the build patches it in), no imports (the premise of both
`E-IMPORT` and the instantiation proof), one memory (so `wasm-merge` needs no
`--enable-multimemory`), and type index *n* taking *n* `i32`s. Names come out
of `repr.rs` itself, so a rename there cannot leave it testing the old one.

**2 — the backend's behaviour, phase 1.** The rest needs the module to *run*:
one case per representation, each a Rocq definition plus a world through the
whole pipeline — `rocq c`, patch the table, `wasm-merge`, `component embed`,
`component new`, `wasmtime run --invoke` — asserting the value returned:

| Case | Pins |
| --- | --- |
| `fun b => b` at `bool` | the tagged immediate, both ways |
| an `enum` of three, returning the first, a middle and the last | nullary ordinals being the declaration index |
| `fun n => n` at `u64`, with 0 and 2⁶³−1 | the boxed primitive int, and the range guard |
| a function returning `None`, and one returning `Some` | `option`'s per-class ordinals |
| a deliberately allocating function | the out-of-memory channel actually firing |
| one export called repeatedly | the [heap reset](#heap-reset) — `mem_ptr` back to its snapshot, `[0, S)` unchanged |
| two and three exports | `MkExports` field offsets (phase 2) |
| `f64` round-trip | the boxed float, once `wasm-prim-floats` lands (phase 4) |

It belongs in `nix flake check` beside 1b — otherwise a CertiRocq bump lands
without anyone noticing the offsets moved — and it cannot be written before the
generator emits adapters. CertiRocq on the per-push path is fine: always
substituted, never built, which the `pin` workflow keeps true.

For the canonical-ABI half, the equivalent check is differential: diff generated
layouts against `cargo component`'s for the matching Rust signature — see
[structured types](#structured-types).

**Expected lifespan.** All of this describes a linear-memory representation with
a bump allocator that never reclaims. WasmGC is the way out, and hosts have it
now ([Endive](https://github.com/bytecodealliance/endive) 1.0 ships it with
native JVM GC integration), but retargeting the backend to WasmGC structs would
invalidate every constant in `repr.rs` and the adapter built on them — and the
canonical ABI is linear-memory based anyway, so components and WasmGC do not yet
compose. Not a near-term option, but the reason `repr.rs` is one file.

---

## <a id="heap-reset"></a>Resetting the heap between calls

The allocator only bumps `glob_mem_ptr`; nothing frees. So every call leaks
everything it allocated, and a component is good for a bounded number of calls
before `out-of-memory` is all it returns. For `plus1` that bound is large; for
[lists](#lists) and [strings](#strings) it is small.

**The adapter should snapshot `glob_mem_ptr` once, immediately after
`main_function` returns, and restore it before every call.** What
`main_function` built — the export closures and their environments — sits below
the snapshot; what a call allocates sits above it and is dead once the result
has been lowered into the canonical ABI. Peak memory becomes the snapshot plus
the largest single call, rather than the sum of all calls.

Why that should be sound: the region below the snapshot is **write-once**. The
generated code only ever writes cells it has just allocated, so if
`main_function` returns at `mem_ptr = S`, every later store is at ≥ S and
`[0, S)` is frozen. Re-entrancy would break it and cannot happen — no
[imports](#imports) means nothing can call back in mid-call. Both are claims to
pin in layer 2 of [representation conformance](#representation), not to assume.

Three things to settle:

- **Where the snapshot lives.** An adapter global, written by whatever calls
  `main_function` — which must happen exactly once, so either a `start`
  function or an initialised flag. Undecided.
- **When the reset fires.** Lowered lists and strings live in our memory and the
  caller reads them after the call returns, so the reset belongs on entry to the
  *next* call, or in `post-return` — never between producing a result and
  handing it over. `post-return` is a [legal no-op](#lists) today because
  nothing is freed; a reset gives it a job.
- **Recovering from out-of-memory.** A failed call leaves garbage above the
  snapshot and `glob_out_of_mem` set. Resetting and clearing the flag would
  make `out-of-memory` a per-call failure instead of a poisoned instance —
  worth having, and it rests on the same frozen-region claim.

---

## Planned

Integers are carried by **Rocq's primitive 63-bit unsigned ints**
(`Uint63.int`), and `u64` is the only supported width. Values ≥ 2⁶³ have no
representation there, so the generator warns at the position where the
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

### <a id="option-result"></a>`option` and `result` as values — `E-OPTION`, phase 2

The outermost `result<T, error>` of an export is [the failure
channel](#failure-channel) and is handled. An `option` or `result` anywhere
else is an ordinary value and needs lowering into, or lifting out of, the heap.

That is much cheaper than the general [structured-type](#structured-types)
work, because their shapes are fixed and their ordinals are known rather than
derived:

| | CertiRocq | WIT |
| --- | --- | --- |
| `None` / `err` with no payload | nullary, unboxed ordinal 0 → immediate `1` | discriminant 0 |
| `Some x` / `ok x` | boxed ordinal 0 → pointer to `[0, x]` | discriminant 1, payload |

So no layout algorithm is needed, just the two cases.

### <a id="structured-types"></a>Records, tuples and variants — `E-STRUCTURED`, phase 3

`record`, `tuple` and `variant`, in any position. (`option` and `result` have
their own, cheaper path — see [above](#option-result).) Two halves:

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

`record` and `tuple` are the ones that matter most, since a user-defined record
is how any multi-field value crosses the boundary. A single-constructor
inductive of arity *n* boxes as `[ord, f₀, …, fₙ₋₁]`, so the Rocq side is
straightforward; the canonical-ABI side is where the care goes.

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

### <a id="integer-widths"></a>Narrow unsigned integers — `E-INT-WIDTH`

`u8`, `u16` and `u32` are rejected for now; `u64` is the only width carried.
They are exactly representable in a 63-bit int on the way *in*, but not on the
way *out*: nothing stops the Rocq side returning an `int` larger than the
declared width, so every lift needs a range check and somewhere to report the
overflow. `u64` needs a guard in one direction only, which is why it came
first.

### <a id="lists"></a>Lists — `E-LIST`

Wanted. `list<T>` is the **only recursive type former the component model
has**, which is what makes it expressible where a user-defined `tree` is not.
A Rocq `list A` maps to `list<A'>` directly — no handles, no resources.

The Rocq side is a cons-cell walk, and the ordinals fall out of
[`repr.rs`](bindgen/src/repr.rs)'s `ordinal::assign(&[0, 2])`:

| | CertiRocq | WIT |
| --- | --- | --- |
| `nil` | nullary, unboxed ordinal 0 → immediate `1` | length 0 |
| `cons hd tl` | boxed ordinal 0 → pointer to `[0, hd, tl]` | one element |

- **Lowering.** The caller writes a contiguous array into *our* memory through
  `cabi_realloc` and hands over `(ptr, len)`. The adapter walks it backwards
  building cons cells at `glob_mem_ptr`, 36 bytes each — `4*(arity+1)+24`, the
  compiler's own `get_ctor_size` (`LambdaANF_to_Wasm.v:131`), so the slack the
  memory invariants expect is preserved.
- **Lifting.** Walk the chain once to count, bump `glob_mem_ptr` for
  `len × size_of(T)` at `T`'s canonical alignment, walk again writing elements,
  return `(ptr, len)`. Two passes, because a bump allocator cannot grow a
  region in place.

What is genuinely new: `cabi_realloc` (align up, allocate, copy
`min(old, new)`, never free — a dozen instructions against a bump allocator); a
`memory.grow` path in the adapter, since it cannot reuse the backend's
`call_grow_mem_if_necessary` (`:300`); `post-return`, legally a no-op because
nothing is ever freed; and element recursion, so `list<list<u64>>` works by
construction.

**[Strings](#strings) pay for almost all of that** — same `cabi_realloc`, same
growth path, same two-pass lift. So lists are best sequenced after strings,
where the remaining delta is element recursion rather than new mechanism.

Two caveats that do not go away:

- **No GC.** A list of *n* elements costs *n* cons cells plus the
  canonical-ABI array, all leaked until `max_mem_pages`. Fine one-shot; a
  repeatedly-invoked list export will hit the ceiling unless the
  [heap reset](#heap-reset) lands first.
- **The memory binding.** `canon lift` has to be bound to the merged module's
  single memory or list reads silently hit the wrong one. This is what makes
  the `--enable-multimemory` flag in `testing/examples/component/plus1/Makefile`
  worth verifying — it matters far more for lists than for scalars, where only
  the result pointer touches memory.

Still not expressible either way: recursive *user* inductives. `list` is the
only recursive former, so a Rocq `tree` has no WIT type short of
[resources](#resources).

### <a id="strings"></a>Strings — `E-STRING`

Wanted. A `string` is bytes under an encoding choice that `canon lift` forces
on you, so it brings real canonical-ABI machinery with it:

- `cabi_realloc`, because a caller lowering a string has to allocate in *our*
  memory. Against a bump allocator this is nearly trivial — allocate and copy,
  never free — but it has to exist, with the ownership and `post-return`
  questions that follow.
- Walking the Rocq value in both directions, and a decision between `string`,
  `list ascii` and a byte array on the Rocq side.

[`list<T>`](#lists) is wanted too, and shares almost all of this machinery, so
the two are best sequenced together — strings first, since one concrete element
type and one encoding is the smaller problem.

### <a id="imports"></a>Imports — `E-IMPORT`

Wanted, and the most expensive item here, because the cost is in the proof
rather than the generator. `mod_imports := []` is baked into the correctness
proof: `module_instantiate_INV_and_more_hold`
(`LambdaANF_to_Wasm_instantiation.v:1234`) instantiates with `[]`, and the store
invariants assume the resulting index layout. Exports-only keeps the existing
proof intact; a world with imports means reworking `instantiation.v`.

An import also runs the ABI the other way round — the adapter becomes the
*callee's* caller, lowering Rocq values out and lifting results back in — so it
needs the lift/lower pair in the mirror direction as well.

Imported *types*, which is what a `use` statement produces, are already fine:
they are just names, and the gate lets them through.

### <a id="frontends"></a>The Lean frontend

Lean needs something to get it into λ□. The
[Peregrine project](https://peregrine-project.github.io/) — a separate effort,
unaffiliated with this one — has that frontend, and routes Rocq and Lean (and
Agda, which is [out of scope](#agda)) through λ□ (LambdaBox) into CertiRocq's
`CodegenWasm`, the backend this uses. So Lean support here would mean invoking
`peregrine` as an external tool, the way the build already invokes `wasm-tools`
and `wasm-merge`.

Almost nothing in the generator is about Rocq, which is what makes this cheap:
only `Iface.v` (a Lean signature in place of a `Module Type`) and `extraction.v`
(a `peregrine wasm prog.ast` invocation rather than `CertiRocq Compile Wasm`).
The adapter, the build rules and the gate are all shared. Three operations
differ per frontend — render a type, render the interface file, drive
compilation — so that is the interface a second one has to implement.

λ□ is untyped — erasure replaces types and proofs with `tBox`, leaving only the
inductive declarations, which is just what `repr.rs`'s ordinal assignment reads.
So WIT must be the source of truth for interface types (it already is), and the
representation is uniform across frontends; what a frontend can still move is
the constructor order erasure produces. λ□ᵗ is no way around that: its
term-level erasure *is* verified in MetaRocq, but the theorem is semantic and
says nothing about the `box_type` annotations, which come from a separate
`erase_type` pass [its authors state is not
verified](https://arxiv.org/html/2108.02995) — relying on a typed target's
checker to reject bad ones, which Wasm has no way to do.

Three things to verify rather than assume:

1. **Primitive integers.** `u64` maps to `Uint63.int`, boxed as 8 bytes.
   Peregrine's Lean frontend turns `Nat` literals into `primInt`, but `UInt64`
   is a separate type, and whether it erases to that primitive or to a structure
   wrapping one decides whether the adapter reads a payload or a constructor
   block.

2. **Constructor ordinals.** `match_ordinals_with_tag` numbers constructors over
   λ□'s list, so a WIT `enum` case reaches an ordinal only via erasure's order.
   Lean's `Bool` is `false | true` like Rocq's — suggestive, not evidence.

3. **What `peregrine wasm` emits.** Same backend, different revision: Peregrine
   pins [`peregrine-project/certirocq#a8dc26fe`](https://github.com/peregrine-project/peregrine-tool/pull/101)
   where this repo pins the 0.9.1+9.1 release. Two revisions agree until they
   don't, and [`checks/representation.py`](checks/representation.py) is the
   instrument — point it at a Peregrine-built module. Needs no generator work.

Peregrine's Lean frontend supports no string literals, floats or arrays, which
overlaps the gate's own gaps — a Lean world is the intersection of two subsets,
so the gate would need to know which frontend it gates for.

The selector exists already: `frontend = "rocq" | "lean"` in `component.toml`,
`--frontend` on the CLI, Rocq by default. `lean` is *rejected* with a pointer
here rather than quietly treated as `rocq`, which would mean handing Rocq
artefacts to someone who asked for Lean.

### <a id="signed-integers"></a>Signed integers — `E-SIGNED`

Only unsigned integers are supported. `s8` … `s64` need a decision first:
`Sint63.int` (clean semantics, but `s64` does not fit in 63 bits) or
`Uint63.int` under a two's-complement reading (fits the bit patterns, but then
the Rocq side computes arithmetic that isn't the WIT arithmetic).

`s8` … `s32` are easy either way. `s64` has the same problem `u64` already has,
one bit short.

---

## Out of scope

### <a id="maps"></a>`map` — `E-MAP`

Everything [lists](#lists) need, plus a key ordering both sides agree on — and
there is no canonical Rocq map type to agree with, so the choice would be the
tool's to impose rather than to follow.

### <a id="f32"></a>`f32` — `E-FLOAT`

Rocq's `PrimFloat` is binary64 only. There is no Rocq type that carries an
`f32` faithfully, and mapping it to a `float` would silently round every value
crossing the boundary in both directions. `f64` is the supported width
([phase 4](#f64)).

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

### <a id="exported-interfaces"></a>Exported interfaces — `E-EXPORT-INTERFACE`

Only top-level `export name: func(..)` is handled. Exporting a named interface
is mostly a naming question — the adapter would export
`ns:pkg/iface#func` — but it interacts with [several exports](#multi-export),
so it waits for that.

### <a id="agda"></a>Agda — out of scope for now

Peregrine also has an Agda frontend into λ□, so the machinery would be shared with
[Lean](#frontends) — the adapter, the build rules and the gate are all
frontend-independent. It is a scope decision, not a technical obstacle: two
frontends is the commitment, and each one costs an interface renderer, a compile
driver and its own answers to the three representation questions. `--frontend`
accepts `rocq` and `lean` only, so asking for Agda is a CLI error rather than
anything subtler.

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

---

## Rejected outright

Not deferred features — there is nothing to build.

### <a id="nothing-to-compile"></a>No exported function — `E-NO-EXPORT`

A world with no exported function gives CertiRocq nothing to compile. Note that
this also fires alongside `E-EXPORT-INTERFACE`, when every export the world has
is a namespaced interface.

### <a id="unresolved"></a>Unresolved world — `E-UNKNOWN`

A type that survived resolution unresolved, which in practice means a missing
dependency package rather than anything about the backend. `wit-parser`
normally reports this first; the check exists so an unresolved type can never
be mistaken for a supported one.
