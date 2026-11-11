# Deferred work

What `componentize-formal` deliberately does not do, and what each item would
cost. The gate lives in [`bindgen/src/support.rs`](bindgen/src/support.rs)
(`check_world`); every rejection prints a stable code and the anchor of its
section below:

```
$ componentize-formal check --wit world.wit
error[E-LIST]: list (list<u32>) is not supported
  --> export total: param `xs`
  = out of scope: needs cabi_realloc and cons-cell walking in both directions, ...
  = see TODO.md#lists
```

Fixtures for every code are in [`bindgen/tests/wit`](bindgen/tests/wit) and
pinned by [`bindgen/tests/support.rs`](bindgen/tests/support.rs), which also
asserts each code is mentioned here.

| Code | Construct | Status |
| --- | --- | --- |
| `E-ARITY-0`, `E-NO-RESULT` | exports with no parameters, or no result | [planned, phase 2](#arity) |
| `E-ARITY-FLAT` | parameters flattening past the canonical ABI's direct limit | [planned, phase 2](#arity) |
| `E-MULTI-EXPORT` | more than one exported function | [planned, phase 2](#multi-export) |
| `E-OPTION-UNIT` | `result` with a payload in neither case | [rejected outright](#option-result) |
| `E-STRUCTURED` | `tuple`, `variant` | [planned, phase 3](#structured-types) |
| `E-RECORD-EMPTY` | a `record` with no fields | [rejected outright](#record-empty) |
| `E-RECORD-UNBOXED` | a `record` with one field | [planned, phase 3](#record-unboxed) |
| `E-INT-WIDTH` | `u8`, `u16`, `u32` | [wanted, not scheduled](#integer-widths) |
| `E-SIGNED` | `s8`, `s16`, `s32`, `s64` | [wanted, not scheduled](#signed-integers) |
| `E-FLOAT` | `f64` | [planned, phase 4](#f64) |
| `E-FLOAT` | `f32` | [out of scope](#f32) |
| `E-LIST` | `list`, fixed-length list | [wanted, not scheduled](#lists) |
| `E-MAP` | `map` | [out of scope](#maps) |
| `E-STRING` | `string` | [wanted, not scheduled](#strings) |
| `E-CHAR` | `char` | [out of scope](#char) |
| `E-FLAGS` | `flags` | [out of scope](#flags) |
| `E-RESOURCE` | `resource`, `own`, `borrow`, methods | [out of scope](#resources) |
| `E-ASYNC` | `async func`, `future`, `stream`, `error-context` | [blocked on WasmCert](#async) |
| `E-IMPORT` | imported functions and interfaces | [wanted, not scheduled](#imports) |
| `E-NO-CHANNEL`, `E-ERROR-TYPE` | an export that cannot report its failures | [required](#failure-channel) |
| `E-NO-EXPORT` | a world with nothing to export | [rejected outright](#nothing-to-compile) |
| `E-UNKNOWN` | a world that did not resolve | [rejected outright](#unresolved) |
| `E-EXPORT-INTERFACE` | `export some:pkg/iface` | [out of scope](#exported-interfaces) |
| — | generated glue for the examples, including a `wasi:http` service | [planned](#generated-examples) |
| — | verified glue | [research project](#verified-glue) |

## Repository layout

| Path | |
| --- | --- |
| [`bindgen/`](bindgen) | the generator, Rust, on `wit-parser` |
| [`bindgen/src/support.rs`](bindgen/src/support.rs) | the support gate |
| [`bindgen/src/repr.rs`](bindgen/src/repr.rs) | CertiRocq's value representation, with sources |
| [`bindgen/src/emit/plan.rs`](bindgen/src/emit/plan.rs) | everything the output depends on, decided once — the only part that can fail |
| [`bindgen/src/emit/`](bindgen/src/emit) | the emitters: `rocq.rs`, `adapter.rs`, `build.rs`, each a pure function of the plan |
| [`bindgen/wit/certirocq-abi.wit`](bindgen/wit/certirocq-abi.wit) | the shipped `error` enum every export returns |
| [`bindgen/tests/wit/{good,bad}`](bindgen/tests/wit) | one fixture per accepted and rejected construct |
| [`examples/`](examples) | hand-written references for the pipeline: [`plus1`](examples/plus1), [`negb`](examples/negb), [`minus`](examples/minus), [`record`](examples/record), [`variant`](examples/variant) (ahead of the gate), and the vendored [`wasi-http`](examples/wasi-http) WIT |
| [`checks/`](checks) | the checks `nix flake check` runs that aren't `cargo test` |
| [`flake.nix`](flake.nix) | the toolchain, the generator package, and every check CI runs |

## <a id="no-silent-failure"></a>No silent failure

A wrong adapter does not announce itself: it reads the wrong bytes and
`wasmtime` hands back plausible garbage. So the gate is deliberately one-sided.

- Every construct is matched explicitly; an unresolved type is itself a
  rejection (`E-UNKNOWN`).
- Every offending position is reported, not just the first.
- Nothing warns today. The one narrowing, `u64` carried by a 63-bit integer, is
  reported through the error channel instead: the adapter range-checks and
  answers `err(out-of-range)`.
- A construct is rejected even when merely declared and never used, because it
  still appears in the component's type.
- Out-of-memory is never swallowed; see [the failure channel](#failure-channel).

### <a id="cofix"></a>Cofixpoints

CertiRocq turns `tCoFix` into a placeholder instead of rejecting it, so a
`CoFixpoint` compiles and can return a wrong answer. WIT has no coinductive
types, so the exposure is an implementation using one internally. Detection
looks cheap: `CertiRocq Show IR` already dumps the IR, and the generator will
drive that invocation anyway.

## <a id="failure-channel"></a>The failure channel — `E-NO-CHANNEL`, `E-ERROR-TYPE`

Every export must return `result<T, error>` with the `error` enum from the
[`certirocq:abi`](bindgen/wit/certirocq-abi.wit) package:

```wit
world wrapper-world {
  use certirocq:abi/errors@0.1.0.{error};
  export plus1: func(val: u64) -> result<u64, error>;
}
```

A bare return type is an incomplete contract: the adapter allocates through the
bump allocator and the backend checks `glob_out_of_mem` after every call, so
every call can fail. `option<T>` is not enough — `plus1` can fail two ways and
`none` cannot say which.

| Failure | Detected |
| --- | --- |
| `out-of-memory` | after the call, from `glob_out_of_mem` |
| `out-of-range` | before the call — a `u64` argument of 2⁶³ or more has no `Uint63.int` |

There is no opt-out: a trapping export takes the instance down and tells its
caller nothing it can act on. The error type is matched by package, version,
interface and type name, so a look-alike enum is rejected (`E-ERROR-TYPE`); see
[versioning](#abi-package-version). The enum never appears on the Rocq side.

### <a id="abi-package-version"></a>The shipped package is versioned

`package certirocq:abi@0.1.0`, and a world must spell the version:
`use certirocq:abi/errors@0.1.0.{error}`. An enum's case order *is* its
discriminant, so appending a case changes what the adapter's `i32` means. The
shipped version is read off
[`certirocq-abi.wit`](bindgen/wit/certirocq-abi.wit), not restated in Rust.
While this is `0.x`, a breaking change takes the minor version; afterwards the
major.

## <a id="representation"></a>Representation conformance

The adapter bakes CertiRocq's representation into loads, stores and offsets, so
the checks pin it in layers:

- [`repr.rs`](bindgen/src/repr.rs) holds every constant with its source, and
  `measured.rs` records what a compiled module actually contained; `const`
  assertions stop the generator compiling if the two disagree.
  `checks/representation.py --update` rewrites `measured.rs` after a deliberate
  backend change.
- `checks.representation` compiles fixtures with the real CertiRocq and reads
  back the module shape, exports, memory/table, constructor encodings and
  one-case-enum erasure.
- `checks.smoke` assembles each example into a component and checks what
  `wasmtime` answers.
- `checks.generated` generates for each example's world and requires the
  hand-written glue token for token, then runs the generated component.
- `checks.roundtrip` composes each conversion in both orders under `wasmtime`,
  exercising the direction function imports will need.
- `checks.backend` compiles a world per representation with the real CertiRocq,
  merges it with its generated adapter and invokes it — the only check where
  both halves of the merge are real. The pairings are chosen so that a wrong
  conversion cannot hide behind itself: `is-green` asks which case it is
  rather than cycling red → green → blue, because a cycle survives both
  conversions being shifted by the same wrong amount; `sub` is not commutative,
  so arguments applied in the wrong order are an answer the check can see.
  Which positions have to be covered is derived from `plan.rs`'s `Repr`, not
  written out, so a fourth representation fails here until a case exists for
  it as a parameter and as a result.

The remaining gap is a hand-written `enum` example: `checks.generated` only
parses the `enum` adapter it emits, while `checks.backend` covers it at run
time. A third example would close that.

### <a id="table-export"></a>The backend exports the table

The adapter reaches the compiled closures through the backend's function table,
exported as `table`. Released CertiRocq does not export it, so `flake.nix` pins
`womeier/certirocq` at `wasm-export-table` head `546dd8a8`: upstream `main`
plus the `MED_table` entry and its `ET_table` obligation. `rocq c` re-checks
that obligation on every build, and `checks/representation.py` fails if the
table is absent.

<a id="pinning"></a>The fork is pinned by revision, not branch name, so a rebase
cannot silently change the tree. `main` needs local builds of WasmCert-Coq
2.2.1 and MetaRocq's `9.1` branch; `flake.nix` carries both, with the two
nixpkgs phases `main` moved.

<a id="per-push"></a>The per-push path substitutes CertiRocq, MetaRocq and
WasmCert-Coq from `womeier.cachix.org`; `.github/workflows/pin.yml` checks the
upstream cache weekly and on changes to the pin. See `flake.nix` and the
workflows for the arrangement.

### <a id="extract-inductives"></a>A remapping the generator cannot see

Constructor immediates come from declaration order (with `bool` the documented
exception), and `CertiRocq Extract Inductives` lets a user's own Rocq file
permute them. The generator does not read that file, so an adapter built from
declaration order would return the wrong case while type-checking and running
cleanly. The real fix is to read the emitted module's constructor constants back
in the build rules — the measurement `checks/representation.py` already makes —
and compare them with what the adapter assumes. Worth doing when the first
generated adapter for a world with its own inductive is built for real.

### <a id="enum-unit"></a>An erased constructor: the one-case `enum`

A one-case `enum` distinguishes nothing, so the backend erases it: the value is
an allocated closure block, not the immediate `1`, and both its projection and
its `match` disappear. The adapter therefore lifts the constant discriminant 0
without reading the value and lowers `2*ord+1` without anything reading it.
`checks.representation` pins the erasure, and `checks.backend` runs a bare
one-case enum, one beside a live field, and a record whose fields are all
erased. If the backend ever stops erasing, the adapter need not change; the
section is the thing to revisit.

## <a id="heap-reset"></a>Resetting the heap between calls

The allocator only bumps `glob_mem_ptr`; nothing frees. Every call leaks what it
allocated, so a component is good for a bounded number of calls before
`out-of-memory`.

The adapter should snapshot `glob_mem_ptr` after `main_function` returns and
restore it before the next call (or in `post-return`, never between producing a
result and handing it over). The region below the snapshot is write-once, so
this is sound while calls cannot re-enter. That changes with
[imports](#imports), where a wrapper allocates mid-program and the timing has to
be re-derived. A reset would also let a failed call recover by clearing
`glob_out_of_mem`, turning `out-of-memory` into a per-call failure. Deferring
costs capacity, not correctness: the limit still arrives through the
[failure channel](#failure-channel).

## <a id="generated-examples"></a>Examples with generated glue

`generate` reproduces [`plus1`](examples/plus1), [`negb`](examples/negb),
[`minus`](examples/minus) and [`record`](examples/record) token for token, but
the committed files remain the hand-written references and `checks.generated`
compares against them. That is deliberate: the references are the independent
oracle. Adapting the examples means adding one whose glue the tool writes — the
target is a `wasi:http` 0.2.x proxy service:

```wit
world proxy {
  include imports;
  export incoming-handler;
}
```

[`examples/wasi-http`](examples/wasi-http) holds the vendored 0.2.8 WIT;
`generate` refuses it today. What has to land first:

| Feature | Where `wasi:http` needs it | |
| --- | --- | --- |
| resources | most of `types`, `io`, streams, pollables — handle tables and destructors | [#resources](#resources) |
| imports | `types`, `outgoing-handler`, `io`, `clocks`, `random`, `cli`; `mod_imports := []` is baked into the proof | [#imports](#imports) |
| `string` | field names, `path-with-query`, `authority`, `error-code` payloads | [#strings](#strings) |
| `list<T>` | `field-value`, `fields`, stream I/O; brings `cabi_realloc` and `post-return` | [#lists](#lists) |
| `tuple` | the `(field-name, field-value)` entry | [#structured-types](#structured-types) |
| `variant` | `method`, `scheme`, `error-code`, `header-error`, `stream-error` | [#structured-types](#structured-types) |
| `u8`, `u16`, `u32` | status codes, alert ids, sizes, poll indices, clock nanoseconds | [#integer-widths](#integer-widths) |
| bare `result` | eight setters return `result` with no payload | [#option-result](#option-result) |
| exported interface | `export wasi:http/incoming-handler`, not a top-level function | [#exported-interfaces](#exported-interfaces) |
| no-result export | `handle` returns nothing, so there is nowhere for out-of-memory | [#arity](#arity) |
| heap reset | every request lowers strings and lists, so calls leak | [#heap-reset](#heap-reset) |

This is two scope reversals (resources, exported interfaces), the proof work
behind imports, three types, a width, and two decisions (payload-less `result`,
and a void export's failure channel). Imports are the expensive part: the proof
`instantiation.v` assumes `mod_imports := []`, and a `wasi:http` handler must
call out to answer anything. Async is not needed for 0.2.x; 0.3.x waits on
WasmCert ([#async](#async)).

## Planned

Integers are carried by Rocq's primitive 63-bit unsigned ints (`Uint63.int`), so
`u64` beyond 2⁶³−1 is a reported failure rather than a silent truncation.

### <a id="arity"></a>Export shape — `E-ARITY-0`, `E-ARITY-FLAT`, `E-NO-RESULT`, phase 2

Several parameters are supported: the backend curries, so the adapter applies
one argument at a time, a box and a `call_indirect` each, reading the next
closure out of `glob_result` in between. Argument boxes are reserved up front
and never reused, and the out-of-memory check runs after every application.
[`examples/minus`](examples/minus) is the reference.

What is left are the two shapes with nothing to apply or nothing to return:

- **No parameters** (`E-ARITY-0`): a nullary export is a value, not a closure,
  so the adapter skips `call_indirect`.
- **No result** (`E-NO-RESULT`): nothing to lift and nowhere to put the
  out-of-memory signal.

**`E-ARITY-FLAT`.** Above `MAX_FLAT_PARAMS` = 16 flat values the canonical ABI
passes a pointer to a tuple instead of one core parameter per WIT parameter.
The limit counts flat values, not parameters: nine `option<u64>` parameters hit
it, and so does a single record of nine `u64` fields. Raised in the planner
rather than the gate because it needs the layout of all parameters together.

Six parameters run end to end (`checks.backend`'s `arity6`). The emitter is
written for any *n* up to the flat limit and the plan is tested to nine, but
nothing past six has been through a real backend.

### <a id="multi-export"></a>Several exports — `E-MULTI-EXPORT`, phase 2

`CertiRocq Compile Wasm` compiles one closed term. For *n* exports the generated
`extraction.v` will compile one *n*-ary constructor holding every closure:

```coq
Inductive Exports := MkExports (plus1 : int -> int) (negb : bool -> bool).
Definition __exports := MkExports Impl.plus1 Impl.negb.
```

A single-constructor inductive of arity *n* is `[ord, f₀, …, fₙ₋₁]`, so export
*i* loads `offset=4*(i+1)`. A Rocq tuple would nest instead of indexing flatly.
`n = 1` skips the wrapper.

### <a id="option-result"></a>`option` and `result` as values — `E-OPTION-UNIT`

One inside an export's channel is an ordinary value, carried to any depth in
either position. Four WIT spellings map onto two Stdlib types:

| WIT | Rocq | discriminant 0 | discriminant 1 |
| --- | --- | --- | --- |
| `option<T>` | `option T` | `none` → `None` | `some` → `Some` |
| `result<T, E>` | `sum T E` | `ok` → `inl` | `err` → `inr` |
| `result<T>` | `option T` | `ok` → `Some` | `err` → `None` |
| `result<_, E>` | `option E` | `ok` → `None` | `err` → `Some` |

`interface.v` writes that mapping next to the signature. A case with no payload
is an immediate; one with a payload is a pointer to `[ord, x]`, and the tag bit
tells them apart. Lowering checks a payload only under the discriminant that
makes it live. A `result` with a payload in neither case is `E-OPTION-UNIT`
([rejected outright](#option-result)); an `enum` of two cases is carried today.

### <a id="structured-types"></a>Records, tuples and variants — `E-STRUCTURED`, phase 3

Records are carried in any position and to any depth; `checks.backend` runs
them. Tuple and variant are what remain.

**Tuple.** A `tuple<u64, u64>` lays out and flattens exactly as a two-field
record, so the plan and emitters would carry it as `Repr::Record` with
positional names. What is missing is a name, and for more than two fields a
shape: Rocq's `prod` nests left, so a three-tuple is two blocks. Either the
generator invents a name per tuple (and structurally equal tuples agree on it)
or it emits the `prod` nesting and the plan grows a representation. Undecided,
and nothing needs a tuple yet.

**Variant.** The harder one. CertiRocq numbers payload-carrying and
payload-less constructors in independent classes, so WIT's single case index
needs a per-case translation table rather than an offset — port
`match_ordinals_with_tag` (`theories/Glue/glue.v:947-960`).
[`examples/variant`](examples/variant) is the reference:

```wit
variant byte-count { unknown, exactly(u64), at-least(u64), too-big }
export add-byte: func(c: byte-count) -> result<byte-count, error>;
```

| WIT discriminant | case | CertiRocq |
| --- | --- | --- |
| 0 | `unknown` | immediate `1` — unboxed ordinal 0 |
| 1 | `exactly(u64)` | `[0, &payload]` — boxed ordinal 0 |
| 2 | `at-least(u64)` | `[1, &payload]` — boxed ordinal 1 |
| 3 | `too-big` | immediate `3` — unboxed ordinal 1 |

No arithmetic on the discriminant produces that column, which is why `enum` can
use `2*d+1` and `variant` cannot. A variant flattens to its discriminant plus
the join of its payloads, so padding for cases that carry nothing must not be
range-checked; the compiled `match` traps on an unknown value, so the adapter
must too.

### <a id="record-unboxed"></a>A record with one field — `E-RECORD-UNBOXED`, phase 3

For one constructor with one field, erasure removes the block: the value **is**
the field, with no projection, no ordinal and no tag test. An adapter that
builds `[0, &field]` reads the header as the value. `checks.backend` found this
the slow way with `record-nested`, which answered `ok({i: {a: 1}, b: 12})`
instead of `ok({i: {a: 2}, b: 12})`.

What it takes is a representation, not a test: a `Repr` variant for a
transparent record, so every match on `Repr::Record` is made to decide what it
does, plus a claim in `repr.rs` measured in `measured.rs` and
`checks/representation.py`. Until then `bad/record-single.wit` and
`bad/record-single-nested.wit` are refused.

### <a id="f64"></a>`f64` — `E-FLOAT`, phase 4

Waiting on the backend's `wasm-prim-floats` branch, where an `f64` is boxed like
a primitive int — a pointer to 8 bytes holding the value — so the lift and lower
are the `int63` shape with `f64.load`/`f64.store`. One thing to confirm when
implementing: whether `canon lift` canonicalises NaN, leaving the adapter only
to move bits.

---

## Wanted, not scheduled

### <a id="integer-widths"></a>Narrow unsigned integers — `E-INT-WIDTH`

`u8`, `u16` and `u32` are exactly representable in a 63-bit int on the way in,
but not on the way out: nothing stops the Rocq side returning a larger `int`, so
every lift needs a range check and somewhere to report overflow. `u64` needs a
guard in one direction only, which is why it came first.

<a id="refinement-types"></a>Refinement types (`{ x : int | x < 2^32 }`) could
move the check to compile time and erase the proof, but the erased arity, the
moved trust boundary and the per-result proof burden remain to be settled. The
erased arity is the sharp one: `sig`'s second field is a `Prop`, so the adapter
has to know whether the erased constructor is `[ord, x, box]` or whether a pass
strips the `Prop` field to `[ord, x]` — the "erased arity is not the Rocq
arity" problem in its sharpest form, to be checked against `CertiRocq Show IR`
rather than assumed, a good first case for the [representation
suite](#representation).

### <a id="lists"></a>Lists — `E-LIST`

`list<T>` is the only recursive type former the component model has, and Rocq's
`list A` maps to it directly — no handles or resources. CertiRocq represents
`nil` as immediate `1` and `cons` as a pointer to `[0, hd, tl]`.

- **Lowering:** walk the caller's `(ptr, len)` array backwards, building cons
  cells at `glob_mem_ptr` (36 bytes each, the compiler's own `get_ctor_size`).
- **Lifting:** count in one pass, bump `glob_mem_ptr` for `len × size_of(T)`,
  write in a second pass, return `(ptr, len)`.

New machinery: `cabi_realloc`, a `memory.grow` path in the adapter,
`post-return`, and element recursion so `list<list<u64>>` works by construction.
[Strings](#strings) pay for most of it, so sequence lists after strings. No GC:
a repeatedly invoked list export hits the ceiling unless the
[heap reset](#heap-reset) lands first. `canon lift` must be bound to the merged
module's single memory.

### <a id="strings"></a>Strings — `E-STRING`

A `string` is bytes under an encoding `canon lift` forces on you, so it brings
`cabi_realloc` and walking the Rocq value in both directions. Against a bump
allocator realloc is allocate-and-copy, never free, but ownership and
`post-return` follow. A Rocq-side choice between `string`, `list ascii` and a
byte array remains. Shares almost all machinery with [lists](#lists); strings
first, since one concrete element type and one encoding is smaller.

### <a id="imports"></a>Imports — `E-IMPORT`

Wanted, and the most expensive item, because the cost is in the proof:
`mod_imports := []` is baked into `LambdaANF_to_Wasm_instantiation.v`, and the
store invariants assume the resulting index layout. A world with imports means
reworking `instantiation.v`.

The mirror-direction lift/lower is already built: both conversions live in
`bindgen/src/emit/convert.rs`, and `checks.roundtrip` runs each pair in both
orders. What is missing is where a wrapper's box comes from: an export reserves
boxes before the program runs, but a wrapper is called mid-program with the
heap in use.

Imported types (`use`) already pass the gate. Importing memory is ruled out
twice: it is not the module the instantiation proof is about, and it would
create a cycle (`wasm-merge` already resolves the compiled module's imports
against the adapter's exports). So imports cost `instantiation.v` and a box,
not the linking shape or the conversion.

### <a id="frontends"></a>The Lean frontend

[Peregrine](https://peregrine-project.github.io/) already has a Lean frontend
into λ□, the same substrate CertiRocq compiles, so Lean support means invoking
`peregrine` as an external tool. Only `interface.v` (a Lean signature) and
`extraction.v` (a `peregrine wasm` invocation) differ; the adapter, build rules
and gate are shared. Three things to verify: whether Lean's `UInt64` erases to
`primInt` or a structure, which constructor ordinals erasure produces, and what
Peregrine's pinned CertiRocq revision emits. Peregrine's Lean frontend lacks
string literals, floats and arrays, so a Lean world is the intersection of two
subsets. The selector (`--frontend lean`) exists and rejects Lean with a
pointer here rather than quietly producing Rocq artefacts.

### <a id="signed-integers"></a>Signed integers — `E-SIGNED`

Only unsigned integers are supported. `s8` … `s64` need a choice between
`Sint63.int` (clean semantics, but `s64` does not fit) and `Uint63.int` under a
two's-complement reading (fits the bit patterns, but the Rocq arithmetic is not
WIT's). `s8` … `s32` are easy either way; `s64` is one bit short.

### <a id="async"></a>Async — `E-ASYNC`

`async func`, `future<T>`, `stream<T>`, `error-context` are 0.3.x shapes; 0.2.x
has none and the guest ABI is synchronous, which is why the `wasi:http` 0.2.x
target can be attempted.

**Async can be confined to the adapter, but only as a synchronous projection.**
The adapter is trusted WAT, so WasmCert's missing async semantics do not stop
async lift/lower and stream/future built-ins from being written there. A
synchronous core function cannot suspend, though, so the adapter can only
bracket the call: materialise the request before it and stream the result out
after it. That makes the Rocq side value-to-value and puts streaming semantics
in trusted glue.

**Faithful 0.3.x needs the substrate first.** The async ABI has to reach the
compiled module, and nothing exists for CertiRocq's compiler to be proved
against until WasmCert models async Wasm. Order: WasmCert, then CertiRocq, then
this tool. The adapter-only route is an interim answer, not support.

---

## Out of scope

### <a id="maps"></a>`map` — `E-MAP`

Everything [lists](#lists) need, plus a key ordering both sides agree on; no
canonical Rocq map type exists to follow, so the choice would be the tool's.

### <a id="f32"></a>`f32` — `E-FLOAT`

Rocq's `PrimFloat` is binary64 only, so mapping `f32` to it would silently
round every value in both directions. `f64` is the supported width
([phase 4](#f64)).

### <a id="char"></a>`char` — `E-CHAR`

WIT's `char` is a Unicode scalar value; Rocq's `ascii` is 8-bit and Stdlib has
nothing matching it.

### <a id="flags"></a>`flags` — `E-FLAGS`

A bitfield with no natural Rocq counterpart. A record of booleans is a different
WIT type with a different ABI.

### <a id="resources"></a>Resources and handles — `E-RESOURCE`

Resources need a handle table and destructors, and there is no GC to hang them
off. This is the escape hatch that would make recursive user inductives
expressible (an opaque handle plus accessor methods), so revisit it if recursive
types become the blocker.

### <a id="exported-interfaces"></a>Exported interfaces — `E-EXPORT-INTERFACE`

Only top-level `export name: func(..)` is handled. Exporting a named interface
is mostly a naming question — the adapter would export
`ns:pkg/iface#func` — but it interacts with [several exports](#multi-export), so
it waits for that.

### <a id="verified-glue"></a>Verified glue

The adapter is **trusted**, exactly as the C backend's glue is. Making it
verified is a research project, not a task:

1. There is no component-model or canonical-ABI formalisation in WasmCert-Rocq,
   so the substrate would have to be built.
2. Verified means no `wasm-merge`: the adapter would be generated into
   `mod_funcs`/`mod_exports`, reversing the separate-adapter design.
3. The theorem — invoking the export on `⟦a⟧` either signals OOM or returns
   `⟦f a⟧` — needs a repeated-call invariant and a memory-layout induction per
   type former.

Roughly a year for a Tier 0/1 subset. The cheaper standalone contribution is the
**canonical ABI as an executable Rocq spec**, differential-tested against
`wasm-tools`.

---

## Rejected outright

Not deferred features — there is nothing to build.

### <a id="nothing-to-compile"></a>No exported function — `E-NO-EXPORT`

A world with no exported function gives CertiRocq nothing to compile. This also
fires alongside `E-EXPORT-INTERFACE` when every export is a namespaced
interface.

### <a id="record-empty"></a>A record with no fields — `E-RECORD-EMPTY`

A record with no fields is a nullary immediate that the canonical ABI flattens
to no core values, so the emitters would need no new case. The component model
refuses it — `record` must have at least one field — and `wasm-tools component
new` reports it four build steps later, at an offset into a custom section. The
gate says it instead, while the field list is still on screen.
`bad/record-empty.wit` pins it. A record carrying no information is one the
export does not need to take.

### <a id="unresolved"></a>Unresolved world — `E-UNKNOWN`

A type that survived resolution unresolved, in practice meaning a missing
dependency package. `wit-parser` normally reports this first; the check exists
so an unresolved type is never mistaken for a supported one.
