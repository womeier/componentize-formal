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
| `E-ARITY-0`, `E-NO-RESULT` | exports with no parameters, or no result | [planned, phase 2](#arity) |
| `E-MULTI-EXPORT` | more than one exported function | [planned, phase 2](#multi-export) |
| `E-OPTION-UNIT` | `result` with a payload in neither case | [rejected outright](#option-result) |
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

## <a id="no-silent-failure"></a>No silent failure

A wrong adapter does not announce itself: it reads the wrong bytes and
`wasmtime` hands back plausible garbage. So the gate is deliberately one-sided.

- Every construct is matched explicitly. Nothing is accepted merely because the
  walk failed to recognise it — an unresolved type is itself a rejection
  (`E-UNKNOWN`).
- Every offending position is reported, not just the first, so one run tells you
  the whole story.
- Anything that *narrows* rather than rejects would get a warning naming the
  position. Nothing warns today, and the warning channel is reporting-only:
  there was a `--deny-warnings` flag to escalate warnings into failures, and it
  was removed because nothing could ever feed it. The one narrowing that exists,
  `u64` carried by a 63-bit integer, is reported as a failure mode through the
  error channel instead -- the adapter range-checks the argument and answers
  `err(out-of-range)`, which is strictly louder than a compile-time note. If a
  narrowing ever turns up that cannot be pushed into the error channel like
  that, `Report::warnings` is still printed by both commands and escalating it
  again is a few lines.
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
Parameter narrow : int -> u32.   (* generated into interface.v *)
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
[`repr.rs`](bindgen/src/repr.rs) with its source, and its unit tests pin the
formulas against each other. Catches a mistake in `repr.rs`, not the backend
moving underneath it --- which is the whole reason for the layers below.

**1a′ — what was measured.**
[`bindgen/src/measured.rs`](bindgen/src/measured.rs) records the immediates a
compiled module actually contained, and `repr.rs` holds itself to them with
`const` assertions. So a representation change that reaches that file stops
the generator compiling rather than changing what it emits:

```text
error[E0080]: evaluation panicked: assertion failed: BOOL_TRUE == crate::measured::BOOL_TRUE
```

The file is generated --- `checks/representation.py --update` rewrites it from
whatever backend is on `PATH` --- so it is a record of a measurement, not a
second opinion. Deciding that a change is intended is the point at which
someone has to run that command.

**1b — the module's shape.** `checks.representation` does this now:
[`checks/representation.py`](checks/representation.py) compiles
[`exports.v`](checks/rocq/exports.v) with the real CertiRocq and reads back
everything visible without running the module — the five export names present,
the table present (the adapter links through it), no imports (the premise of both
`E-IMPORT` and the instantiation proof), one memory (so `wasm-merge` needs no
`--enable-multimemory`), and type index *n* taking *n* `i32`s. Names come out
of `repr.rs` itself, so a rename there cannot leave it testing the old one.

It also reads back the **constructor encoding**, which was filed under layer 2
on the assumption that it needed the module to run. It does not: a function
that ignores its argument and returns a nullary constructor compiles to that
constructor's immediate and nothing else, so
[`constructors.v`](checks/rocq/constructors.v) compiles five of them and the
check reads the constant out of each wrapper. That pins `bool`'s swapped pair
and, as a control, a three-case inductive that is *not* swapped.

Constant functions, not `negb`: an involution hides a swapped encoding
completely, because lifting and lowering with the same wrong mapping cancels
out and every answer still looks right.

Finding that wrapper is the part that moved with the CertiRocq below. The
module now exports *every* function the pipeline generated, each under a
mangled name --- `_0__reccall_wrapper_193`, `_1__f_case_known_194` --- where
the release exported the definition's wrapper and nothing else, so the export
list no longer says which function belongs to the definition. The check reads
the code pointer out of the closure `main_function` leaves in `result` and
follows that, which is the path the adapter takes at run time; the `elem`
section maps function index to table index one to one, so the one constant
serves as both. The immediates that came back were identical to what
`measured.rs` already recorded, `bool`'s swap included --- the export shape
moved, the encoding did not.

A constructor that *takes arguments* carries an ordinal rather than an
immediate, and compiles to a body full of the allocator's own constants with
nothing marking which one it is. Those are read by diffing instead: two
functions differing in nothing but the constructor they build must compile to
bodies differing in exactly one constant. `mixed := m_first | m_boxed_a _ |
m_boxed_b _ | m_last` is the type that matters, because its two boxed
constructors are ordinals 0 and 1 --- the same numbers its two nullary ones
have --- so an adapter going by the WIT discriminant names a different
constructor while type-checking perfectly. That is what
[`examples/variant`](examples/variant) is built on, and the measurement is
what holds `repr::ordinal::assign` to the backend rather than to itself.

**1c — the components answer.** `checks.smoke`
([`checks/smoke.py`](checks/smoke.py)) assembles each example under
[`examples/`](examples) into a component and compares what `wasmtime` prints
against expectations written out in full. It links against the examples'
`stub/`, so it needs no Rocq and runs on every push; what holds the stub to
the real backend is 1b above, and layer 2 below.

**1d — the generator emits that.** `checks.generated`
([`checks/generated.py`](checks/generated.py)) generates for each example's
world and requires the result to be the hand-written adapter *token for token*,
comments and line breaks excepted, then assembles the generated adapter against
the same `stub/` and requires the same answers. The examples are never
regenerated: they were written, built and run before the emitter existed, which
is the only reason they are evidence about it. It also generates for every world
in [`bindgen/tests/wit/good`](bindgen/tests/wit/good) and parses the result,
since those fixtures are the rest of the shapes the gate accepts, and runs
`make -n` over the generated build rules.

The first thing this check found was the emitter passing a boxed `u64` argument
by value instead of by address --- a component that assembles, runs, and
answers nonsense.

What it does not cover is the `enum` branch of the emitter, because no example
under `examples/` has an `enum` parameter: the two that exist take a `u64` and
a `bool`, so `checks.generated` only parses the adapter it generates for the
`enum` fixtures. Layer 2 below now assembles `enum` components and reads
answers out of them, which is the risk; what is still missing here is the
*reference* — a hand-written `enum` example, written before the emitter and
held to token for token. A third example closes that.

**1e — the conversions are inverses.** `checks.roundtrip`
([`checks/roundtrip.py`](checks/roundtrip.py)) takes the pair of conversions
each representation has — canonical ABI to CertiRocq's value and back, both
out of [`bindgen/src/emit/convert.rs`](bindgen/src/emit/convert.rs) — composes
them in both orders in a module of its own, and runs every order on every
value under `wasmtime`, requiring the value back unchanged.

This is narrower than 1c and 1d and catches something neither can. An export
adapter uses each direction in exactly one position, so a component's answers
say nothing about the other order — the one a function import will need
([#imports](#imports)), where the wrapper takes Rocq arguments out and brings
a host result back in. And because the harness links against nothing, it runs
the `enum` branch the paragraph above says no example reaches: `enum.wit`'s
three cases go through `2*ord+1` and back for real, with the Rocq-side values
recomputed in the check rather than read from the generator. What it still
does not do is assemble an `enum` component and read an answer out of it, so a
third example is still what closes 1d.

**2 — the backend's behaviour.** The rest needs the module to *run*, and now
does. `checks.backend` ([`checks/backend.py`](checks/backend.py)) generates a
world per case, compiles it with the real CertiRocq, merges it with its own
generated adapter and invokes the component, asserting the value that comes
back — `rocq c`, `wasm-merge`, `component embed`, `component new`, `wasmtime
run --invoke`, driven through the generated `Makefile` rather than around it.

This is the one step no other check takes. A stub under `examples/*/stub` is
written against the same `repr.rs` the adapter is, so it agrees with the
adapter by construction, and 1c and 1d can only catch an adapter that
disagrees with itself. Here the other half of the merge is whatever CertiRocq
emitted.

| Case | Pins | |
| --- | --- | --- |
| `succ` at `u64`, with 0, 2⁶³−1, 2⁶³ and 2⁶⁴−1 | the boxed primitive int in both positions, the modular wrap, and the range guard at both ends of the half a `u64` has and `Uint63.int` has not | runs |
| `to-int`, `is-zero` | the tagged immediate, each direction paired with a result of another representation | runs |
| `is-green`, `colour-of` | nullary ordinals being the declaration index — first case, a middle one, the last | runs |
| `sub` on two `u64` | one application per argument, in declaration order ([#arity](#arity)) | runs |
| any of them with the heap capped at the module's declared minimum | the out-of-memory channel firing on the real allocator, not on a poisoned bump pointer | runs |
| `unwrap-or`, `halve` | `option`'s two kinds of case in one type — the immediate `None` and the boxed `Some` — one position each | runs |
| `flip` on a `result<u64, bool>` | `inl` and `inr`, the ordinals a swap upstream would exchange, mapped both ways in one call | runs |
| `dig` on an `option<option<u64>>` | a sum inside a sum: the inner constructor built where the outer one's field is written, the payload lifted through two blocks | runs |
| `shade` on an `option<colour>` | a sum whose payload is an *immediate*: the one shape where a boxed constructor's field is `2*ord+1` rather than a pointer, permuted so no case answers as another | runs |
| one export called repeatedly | the [heap reset](#heap-reset) — `mem_ptr` back to its snapshot, `[0, S)` unchanged | with the reset |
| two and three exports | `MkExports` field offsets | with [several exports](#multi-export) |
| `f64` round-trip | the boxed float | with `wasm-prim-floats` (phase 4) |

The pairings are chosen so that a wrong conversion cannot hide behind itself.
`is-green` returns a `bool` rather than cycling red → green → blue, because a
cycle survives both conversions being shifted by the same wrong amount — a
rotation commutes with a rotation — while asking which case it is does not;
`sub` is not commutative, so applying the arguments in the wrong order is an
answer the check can see. Which positions have to be covered is derived from
`plan.rs`'s `Repr`, not written out, so a fourth representation fails here
until a case exists for it as a parameter and as a result.

The out-of-memory row needs no allocating Rocq program, which would have to
ask for the better part of two gigabytes: the component is invoked again with
the host granting exactly the pages the module declares as its minimum, so the
first `memory.grow` the allocator asks for fails, the backend sets
`out_of_mem`, and the adapter has to answer `err(out-of-memory)` rather than
trap or return whatever `result` holds.

What it costs is a CertiRocq on the per-push path, which
`checks.representation` already put there and the cache this project owns is
what pays for: see [the per-push path](#per-push). The six cases take seconds
once CertiRocq is substituted — they are four-line definitions.

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

### <a id="table-export"></a>The backend exports the table

The adapter reaches the compiled closures through the backend's own function
table, so the module has to export it. The name is plain `table`, matching the
rest of the backend's exports (`memory`, `result`, `mem_ptr`) rather than
LLVM's `__indirect_function_table`: a `MED_table 0%N` entry next to the
`MED_mem 0%N` one in `LambdaANF_to_Wasm.v`. Before it existed the build
`sed`-ed the export in between `rocq c` and `wasm-merge` --- the one step that
rewrote the backend's output instead of wrapping it. Now the generated build
rules link the backend's own bytes.

**Which CertiRocq.** `womeier/certirocq` at `wasm-export-table`, pinned to its
head `546dd8a8`: upstream `main` plus eleven lines, adding the entry and
discharging the `ET_table` obligation the instantiation proof then carries,
nothing admitted. `rocq c` re-checks that obligation on every build, so a
rebase that invalidates the proof fails the build instead of quietly losing
it. No released CertiRocq exports the table and neither does `main`. Pinned by
revision rather than by branch name, here and for MetaRocq below, because a
branch pin follows the next rebase on its own --- a tree that compiled
yesterday stops compiling with nothing in `flake.nix` having changed.

**What that costs.** `main` wants two dependencies newer than the nixpkgs pin
has, so both become local builds:

- **WasmCert-Coq 2.2.1**, because `LambdaANF_to_Wasm_utils.v:1173` uses
  `write_bytes_gen_preserve_length`, which 2.2.1 adds and 2.2.0 does not have
  anywhere. Cheap: it needs nothing newer than the pin already provides.
- **MetaRocq at the head of its `9.1` branch** (`51b14e4b`), because
  `Compiler/metarocq_pipeline.v` imports
  `MetaRocq.Erasure.EImplementLazyForce`, which exists in no release ---
  the pinned 1.5.1-9.1 set has `EImplementBox` and nothing else. Expensive:
  hours from source, and in no cache this flake can reach, including
  `metarocq.cachix.org`, whose store names come from coq-nix-toolbox's
  overlays rather than this nixpkgs revision's expression.

The nixpkgs expression also describes `0.9.1+9.1`, so two of its phases are
wrong for `main` and both would fail only after all of `theories` had
compiled: the manifests are generated by `plugins/manifests/generate.py`,
which needs a `python3` it does not bring, and the plugins moved to
`plugins/plugin` and `plugins/cplugin`, which its `installPhase` does not
know. `flake.nix` corrects those two, following CertiRocq's own
`.nix/coq-overlays/CertiRocq/default.nix`.

The consequence for `checks/cache_hits.py` is that it can no longer speak
for MetaRocq, the build it existed to protect. It checks what is left ---
CompCert, and the inputs MetaRocq is built from --- and says in its docstring
what it gave up.

**It compiles and runs.** All nine checks pass, and every example answers
correctly through `make run`: CertiRocq's own module linked to the adapter,
which could not be assembled at all before the export existed. Two things
moved with the pin --- the plugin's flags are double-dashed now, `-file`
replaced by `--output`, and the module's export shape changed, which
[1b](#representation) covers.

Which CertiRocq a user compiles with is still an assumption the generator
cannot check at run time, so it is checked where assumptions about the backend
belong: `checks/representation.py` fails if the table is not exported, naming
the input as the thing to move, and the generated build rules guard the link
step the same way.

#### <a id="per-push"></a>The per-push path, bought with a cache of our own

The first way out was taken. `womeier.cachix.org` is this project's own
Cachix, declared in `flake.nix`'s `nixConfig` beside the three upstream ones,
and `.github/workflows/ci.yml` both reads and writes it through
`cachix/cachix-action`. CertiRocq, MetaRocq and WasmCert-Coq are substituted
from it like anything else, so a per-push run is minutes again --- and so is
the first `nix develop` of anyone who accepts the flake's `nixConfig` and can
have it honoured, which the old arrangement never covered at all: it only ever
made CI cheap.

What did not survive is the assertion. `nix build --max-jobs 0 .#certirocq`
existed to turn a collected cache into a failure in seconds instead of a
MetaRocq build on a runner, and with the three moved packages uncached
upstream it failed by construction. It is gone from `ci.yml`, which now runs
`nix flake check` twice --- once as eval over every system, once for real ---
and nothing else. So the per-push gate no longer asserts anything about
caches. A cold project cache shows up as a long run bounded by
`timeout-minutes: 330`, not as a fast failure. That trade is deliberate: fast
failure was only ever available while upstream held everything, and the table
export is what stopped it holding.

The checking moved to `.github/workflows/pin.yml`, which is where it belongs
--- it is a claim about the pin, not about a push. Mondays at 04:17 UTC, on a
push touching `flake.nix`, `flake.lock` or the workflow, and on demand, it
runs `checks/cache_hits.py` against the *upstream* substituters only, named in
the script and passed with `--no-accept-flake-config` so that our own copy
cannot answer for the pin and hide an upstream GC behind it. Then `nix build
.#certirocq`, whose closure the cachix step pushes: nothing keeps a Cachix
path alive but being pushed, so that step is both the refill and the check
that what CI depends on is still there.

What it costs is a cache, a token (`secrets.CACHIX_AUTH_TOKEN`), and that job
to keep it warm. A fork PR has no token; `cachix-action` then substitutes
without pushing, which is the behaviour we want, since making the missing
secret fatal would fail every fork PR.

The other two ways out are unchanged, and both are still better than paying
for this one:

- **Split the gate.** Only `representation` needs Rocq; the other six are the
  `bindgen` shell and could stay per-push on their own. Cheap, and it gives up
  the one check that holds `repr.rs` to the real backend --- unnecessary while
  the cache holds, and the thing to reach for if it stops.
- **Get the export upstream.** Merged and released, CertiRocq is a nixpkgs
  cache hit again, which retires this cache, all three source builds, and the
  ground `cache_hits.py` gave up, in one move. Slowest, and the only one that
  removes the problem instead of paying for it.

### <a id="extract-inductives"></a>A remapping the generator cannot see

The constructor immediates the adapter contains come from declaration order,
and `bool`'s come from the one documented exception to it. Both are
*configurable*. `CertiRocq.Plugin.Loader` --- the three-line file that
`From CertiRocq.Plugin Require Import CertiRocq` loads --- sets the exception
up with

```coq
CertiRocq Extract Inductives [ bool => "bool" [ 1 0 ] ].
```

and that command is available to anyone. A world's `enum colour` compiled in a
file that also says `CertiRocq Extract Inductives [ colour => "colour" [ 2 1 0 ] ]`
would permute the ordinals under an adapter built from declaration order, and
every call would return the wrong case while type-checking and running cleanly.

Nothing on the WIT side can detect this: the remapping lives in the Rocq file
the user writes, which the generator does not read. Three options, none taken
yet:

- **Say so.** The generated `interface.v` could carry the warning, which costs
  nothing and relies on someone reading it.
- **Check the output.** The immediates are visible in the compiled module, so
  the build rules could read the constructor constants back and compare them
  against what the adapter assumes --- the same measurement
  [`checks/representation.py`](checks/representation.py) already makes, applied
  to the user's own types rather than to fixtures. This is the real answer.
- **Own the file.** `extraction.v` is generated, so it could be the only place
  the command may appear and the generator could refuse any other use of it.
  That means parsing Rocq, which is a bad trade.

The middle one is worth doing when the first generated adapter for a world with
its own inductive gets built for real.

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
`[0, S)` is frozen. Re-entrancy would break it, and today it cannot happen ---
no [imports](#imports) means nothing can call back in mid-call. Both are claims
to pin in layer 2 of [representation conformance](#representation), not to
assume.

**The re-entrancy half expires when imports land.** Once the compiled module
can call out through an adapter wrapper, the host can be on the stack while a
call is still in flight, and that wrapper allocates in order to lower its
arguments. Write-once survives --- those allocations are above `S` like any
other --- but the timing question below does not: a reset on entry to a call is
wrong while an outer call is live. Whatever answer that question gets has to be
re-derived for a re-entrant stack, not inherited.

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

**Deliberately not in the first generated adapter.** Deferring it costs calls,
not correctness: without the reset a component answers fewer times before
`out-of-memory`, and that limit arrives through the
[failure channel](#failure-channel) like any other failure — a caller is told,
never handed a wrong answer. So it is a capacity improvement resting on a
frozen-region claim that layer 2 has to pin anyway, and it can land after the
first component runs end to end rather than blocking it.

---

## Planned

Integers are carried by **Rocq's primitive 63-bit unsigned ints**
(`Uint63.int`), and `u64` is the only supported width. Values ≥ 2⁶³ have no
representation there, so the generator warns at the position where the
narrowing happens and the adapter guards rather than truncating.

### <a id="arity"></a>Export shape — `E-ARITY-0`, `E-NO-RESULT`, phase 2

**Several parameters are supported now.** `E-ARITY-N` is gone, and `func(a: T,
b: U, ...) -> result<V, error>` goes through. What settled it was a
measurement, not an argument: the open question had been whether uncurrying
changes the arity of the closure reachable from `main`'s result, and it does
not. A compiled `int -> int -> int` has four functions and a table of four,
and every one of them has the `(env, arg)` closure type, so there is no 2-ary
core function to call. Iterated application is the only shape there is.

So the adapter applies one argument at a time — a box and a `call_indirect`
each, with the next closure read out of `glob_result` in between, which is
what the C glue's `make_call` (`theories/Glue/glue.v:1229`) does.
[`examples/minus`](examples/minus) is the reference: hand-written, measured,
answering correctly through `make run` against the real backend and not only
the stub, and now emitted token for token by the generator. Its README has the
numbers.

Two things the measurement added to the plan that was written here before it.
The argument boxes have to be reserved up front and never reused: the first
application captures the adapter's box *by address*, and a later application
reads through that pointer, so an adapter that allocated one box per argument
in turn would compute `b - b`. And the out-of-memory check belongs after
*every* application, not just the last — a failure in the middle leaves the
*previous* closure in `glob_result`, and applying the next argument to it is a
`call_indirect` into a body expecting a different shape. That is a wrong
answer today and a trap once [structured types](#structured-types) land, where
a body loads an ordinal and runs `unreachable` on anything else. The flag
being sticky is what makes this a safety check rather than a reporting one.

**What is left** is the two shapes with nothing to apply or nothing to return,
each needing its own adapter prologue:

- **No parameters.** A nullary export is a value, not a closure, so
  `main_function` leaves the result itself in `glob_result` and the adapter
  skips `call_indirect` entirely.
- **No result.** Nothing to lift, and nowhere to put the out-of-memory signal.

Only *n* = 2 has been run end to end. The emitter is written for any *n* and
the generated adapter for three parameters parses, but three has not been
through a real backend.

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

### <a id="option-result"></a>`option` and `result` as values — `E-OPTION-UNIT`

The outermost `result<T, error>` of an export is [the failure
channel](#failure-channel). An `option` or `result` anywhere else is an
ordinary value, and **both are carried, to any depth** — in a parameter, or in
the ok half of the channel. Nothing in the gate counts levels, and nothing in
the emitters special-cases a nested one: the flattening, the heap cells, the
range checks' guards and the discriminant traps are each recursive.

This is much cheaper than the general [structured-type](#structured-types)
work, because the shapes are fixed and the ordinals are known rather than
derived. Four WIT spellings land on two Stdlib types, and which constructor
carries which case is a table rather than a formula — the `result` rows map
`ok` to opposite constructors:

| WIT | Rocq | discriminant 0 | discriminant 1 |
| --- | --- | --- | --- |
| `option<T>` | `option T` | `none` → `None` | `some` → `Some` |
| `result<T, E>` | `sum T E` | `ok` → `inl` | `err` → `inr` |
| `result<T>` | `option T` | `ok` → `Some` | `err` → `None` |
| `result<_, E>` | `option E` | `ok` → `None` | `err` → `Some` |

The generated `interface.v` writes that mapping next to the signature, because
nothing else in the pipeline would tell you that the ok half of a `result<T>`
is `Some x`.

A `result` with a payload in **neither** case is `E-OPTION-UNIT` rather than
deferred work: there is no value to carry and no Stdlib sum to carry it in. An
`enum` of two cases says the same thing and is carried today — and `bool`, the
obvious thing to reach for instead, is the one inductive whose ordinals are
swapped, so carrying it there would exchange the two cases.

Two facts the emitter rests on, both measured by
[`checks/representation.py`](checks/representation.py) rather than assumed:

| | CertiRocq |
| --- | --- |
| a case with no payload | nullary, unboxed ordinal 0 → immediate `1` |
| a case with one | boxed ordinal 0 or 1 → pointer to `[ord, x]` |

So a sum is a pointer *or* an immediate depending on which case it holds, and
the tag bit (`x & 1`, clear for a pointer) tells them apart — the same test
the backend's own `match` makes. With one case of each kind that bit is the
whole lift; with both boxed, as a `result<T, E>` is, the ordinal in the first
cell picks the case and an immediate arriving there is impossible rather than
merely unexpected, so it traps.

Lowering is guarded the other way round. The canonical ABI zero-pads the flat
slots of the case that was *not* sent, so a `u64` payload's range check runs
under the discriminant that makes the payload live — a failure concluded from
padding would be invented. Nested, that is *every* discriminant on the way
down, not only the innermost one.

What bounds the nesting is the canonical ABI's own flattening rather than a
number here, and the plan cross-checks its flattening against `wit_parser`'s
before emitting anything.

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

**There is a target to build against.** [`examples/record`](examples/record) is
the whole pipeline for

```wit
record span { start: u64, byte-length: u64 }
export grow: func(s: span) -> result<span, error>;
```

hand-written, measured against a module the real CertiRocq compiled, and
running --- `checks.smoke` holds it to its six answers. `checks.generated`
holds the generator to *refusing* it, with `E-STRUCTURED`, until this section
is done, so the day the gate starts accepting records is the day that check
asks for the emitter to match the example token for token.

Four things the record example settles, none of them guessable from the
sources:

- A WIT record of two `u64`s **flattens** into two core `i64` parameters, so
  nothing arrives by pointer and the adapter has to *build* the Rocq value:
  a box per field, then `[0, &f₀, &f₁]` pointing at them. Each cell holds a
  pointer, because a primitive int is itself boxed.
- The backend reads each field through **its own table function**, which traps
  unless the pointer is even and the ordinal is 0. So the ordinal has to be
  written, not left to whatever the heap held --- and both of the example's
  stubs arrive with a dirty heap, because over a zeroed page an adapter that
  forgot it would pass.
- The compiled body **reuses the caller's box** for a field it does not change,
  which is why a record returned to the adapter can point back into the
  adapter's own reservation. Nothing may assume the returned value is disjoint
  from the argument.
- The second out-of-memory check earns its keep here: a heap that runs out
  *during* the call leaves the closure in `result`, and without the check the
  component answers `ok({start: 0, byte-length: 0})`. `plus1`'s stub cannot
  reach that branch.

[`examples/variant`](examples/variant) is the other half, for

```wit
variant byte-count { unknown, exactly(u64), at-least(u64), too-big }
export add-byte: func(c: byte-count) -> result<byte-count, error>;
```

and it is the one that makes the per-class ordinals visible:

| WIT discriminant | case | CertiRocq |
| --- | --- | --- |
| 0 | `unknown` | immediate `1` --- unboxed ordinal 0 |
| 1 | `exactly(u64)` | `[0, &payload]` --- boxed ordinal 0 |
| 2 | `at-least(u64)` | `[1, &payload]` --- boxed ordinal 1 |
| 3 | `too-big` | immediate `3` --- unboxed ordinal 1 |

No arithmetic on the discriminant produces that column, which is why `enum`
could get away with `2*d+1` and `variant` cannot: the adapter writes the cases
out one at a time, in both directions, and the tag bit is what says which class
a number belongs to. Lift `exactly` with ordinal 1 instead of 0 and the
component answers `add-byte(exactly(41)) = ok(at-least(42))` --- it assembles,
runs, and lies. Two further things it settles:

- A variant **flattens** to its discriminant plus the join of its payloads, so
  a four-case variant with two `u64` payloads arrives as an `i32` and an `i64`
  --- and for the two cases that carry nothing, that `i64` is not part of the
  value, so it must not be range-checked.
- The compiled `match` **traps** on a value that is neither of the immediates
  nor either ordinal, so the adapter's lowering does too. An adapter more
  forgiving than the module it is linked into turns a representation that has
  drifted into a wrong answer instead of a stop.

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
*callee's* caller, lowering Rocq values out and lifting results back in — so
it needs the lift/lower pair in the mirror direction as well.

**That half is already built, and already run.** It is not extra work waiting
on the proof: both conversions live in `bindgen/src/emit/convert.rs`, and the
only difference between an export's glue and an import wrapper's is the order
they are composed in. An export takes a parameter canonical → Rocq and hands a
result back Rocq → canonical; a wrapper takes its arguments the second way and
its answer the first. So `componentize-formal roundtrip` emits a module
composing each pair in *both* orders and `checks.roundtrip` runs it under
`wasmtime`, requiring the value back unchanged — over both ends of
`Uint63.int`'s range, both booleans, and every case of an `enum`. The
direction no emitted adapter uses is tested before there is anything that uses
it, which is the part of this item that could otherwise rot unnoticed: the
gate refuses imports, so nothing else would exercise it.

What is *not* built is where a wrapper's box comes from. Nothing in
`convert.rs` allocates — a boxed value needs a cell and is handed the address.
An export reserves every box up front, off the bump pointer, before the
program runs ([#arity](#arity)); a wrapper is called mid-program with the heap
in use, and finding a cell there is a question about imports rather than about
the conversion.

Imported *types*, which is what a `use` statement produces, are already fine:
they are just names, and the gate lets them through.

**Nor can the emitted module import its memory**, which is the tempting fix
when the two core modules look like they depend on each other. Core modules
inside a component may import memories from other core instances, so unlike at
the component boundary --- where memory is not something a component can import
at all --- this one is expressible. It is still ruled out twice over.

`mod_imports := []` is the same premise as above: a module given a memory
import is not the module the instantiation proof is about, and the store
invariants are stated over a memory the module itself defines.

And it would *create* the cycle rather than remove it. The adapter already
imports `memory`, `mem_ptr`, `result`, `out_of_mem`, `main_function` and the
table from the compiled module; having the compiled module import its memory
back from the adapter makes each need the other, and a component's instance
graph has to be acyclic. The shape that would work is a third module owning
nothing but the memory, depending on no one and exported to both --- which buys
nothing here, because `wasm-merge` runs before `component embed` and the merged
module owns the memory, the globals, the table and the adapter's functions with
no imports left to resolve.

Nor does the direction need to stay one-way once there *are* imported
functions, which is the case that looks like it should cycle: the compiled
module imports the lowered wrapper from the adapter while the adapter imports
the memory and globals from the compiled module. `wasm-merge` is a link step,
so it resolves each import against the other module's exports and both edges
disappear --- checked, with that exact pair of stand-in modules: the merge
succeeds and the only import left is the host function, which is what
`component new` satisfies from the world's imports. So imports cost
`instantiation.v` and a box to put an argument in — not the linking shape,
and not the mirror-direction lift/lower.

### <a id="frontends"></a>The Lean frontend

Lean needs something to get it into λ□. The
[Peregrine project](https://peregrine-project.github.io/) — a separate effort,
unaffiliated with this one — has that frontend, and routes Rocq and Lean (and
Agda, which is [out of scope](#agda)) through λ□ (LambdaBox) into CertiRocq's
`CodegenWasm`, the backend this uses. So Lean support here would mean invoking
`peregrine` as an external tool, the way the build already invokes `wasm-tools`
and `wasm-merge`.

Almost nothing in the generator is about Rocq, which is what makes this cheap:
only `interface.v` (a Lean signature in place of a `Module Type`) and `extraction.v`
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
   where this repo pins CertiRocq `main` at `943b52ae`. Two revisions agree
   until they don't, and [`checks/representation.py`](checks/representation.py) is the
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
