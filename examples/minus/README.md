# `minus`

Two parameters, which means the adapter applies them one at a time:

```wit
export minus: func(a: u64, b: u64) -> result<u64, error>;
```

These files were written by hand while the gate still refused this world
(`E-ARITY-N`), measured against the real backend, and run — as the shape the
generator would have to emit. It emits exactly them now, so the glue committed
here is its output, held to the byte by `checks.generated`; see
[`plus1`](../plus1) for which files those are
([`TODO.md#arity`](../../TODO.md#arity)).

```console
$ nix develop
$ cd examples/minus && make run
minus(10, 3) = ok(7)
minus(3, 10) = ok(9223372036854775801)
minus(0, 1) = ok(9223372036854775807)
minus(9223372036854775807, 9223372036854775807) = ok(0)
minus(9223372036854775808, 0) = err(out-of-range)
minus(0, 9223372036854775808) = err(out-of-range)
$ make run-stub-oom
minus(10, 3) with the heap exhausted between the applications = err(out-of-memory)
```

That is `make run` — CertiRocq's own module, linked through the table export,
not the stub. `make run-stub` prints the same six lines.

**Why subtraction.** It is the cheapest two-argument function whose answer
depends on which argument is which. An adapter that applied the two arguments
in the wrong order would pass every test an `add` example could write, because
`a + b` is right either way; here it answers `ok(9223372036854775801)` where
`ok(7)` was wanted. The same reasoning as [`record`](../record)'s `grow`
touching its two fields differently.

The other four lines: `minus(0, 1)` wraps to 2⁶³-1 because `Uint63` wraps, as
in [`plus1`](../plus1), and the two `err(out-of-range)` lines differ only in
*which* argument is out of range. Neither argument is exempt, so each is its
own check, and one firing is no evidence about the other.

## What is new here, over `plus1`

**The argument loop.** A compiled `int -> int -> int` is not a two-argument
function. It is two one-argument functions, and the adapter walks them:

```
            box the argument          apply it                  then
  a     →   [a] at $ret+16      →     call_indirect        →    result holds a closure
  b     →   [b] at $ret+24      →     call_indirect        →    result holds the answer
```

Each application reads `[closure+4]` for the table index and `[closure+8]` for
the environment, exactly as `plus1`'s single one does. The closure for the
next application comes out of `result` — the same global, read the same way,
which is what makes this loop the same code *n* times for any arity.

**The argument boxes have to outlive their application.** The first
application does not copy `a`; it captures the adapter's own box *by address*,
and the second application reads through that pointer. So all of the
reservation is taken up front and none of it is reused between applications.
An adapter that allocated one box and overwrote it per argument would compute
`b - b`.

**The out-of-memory check belongs after every application, not just the last.**
Applying one argument of two allocates — an environment holding `a`, and the
closure for the next application — so the middle of the loop is a place the
flag can be set with `result` still holding the closure from before.
[`stub/oom.wat`](stub/oom.wat) is a heap that runs out exactly there, which is
a branch neither [`plus1`](../plus1)'s stub nor [`record`](../record)'s can
reach.

What that check buys is worth stating precisely, because it is not what it
looks like. `out_of_mem` is only ever set and never cleared, so the check after
the *last* application would see this failure too — the answer would still be
`err(out-of-memory)`. What the middle check prevents is the `call_indirect`
that an unchecked adapter would make next, through a closure expecting an
argument of the shape the *previous* application took. Here both wrappers take
a boxed int, so it would merely compute the wrong thing; a body that loads an
ordinal and runs `unreachable` on anything else
([`TODO.md#structured-types`](../../TODO.md#structured-types)) turns it into a
trap — a component that dies instead of answering.

## Measured, not inferred

The representation above was read off a module the real CertiRocq compiled, not
worked out from the sources. For `fun a b => (a - b)%uint63` it says:

- **Nothing is uncurried.** The module has four functions and a table of four,
  and every one of them has type `(param i32 i32)` — the `(env, arg)` closure
  type from `repr.rs`. There is no 2-ary core function to call, so iterated
  application is not a choice between designs; it is the only shape there is.
  This is the open risk [`TODO.md#arity`](../../TODO.md#arity) asked to have
  settled before the generator was written against it.
- **`main_function` builds the same closure as in `plus1`**: `[0, 3, 1]` —
  ordinal, table index, and an environment that is an immediate rather than a
  pointer, because nothing is captured yet and the first wrapper ignores it.
- **The first application allocates twice and computes nothing**: 8 bytes for
  the environment `[0, &a]`, then 12 for the closure `[0, 2, env]`, which it
  leaves in `result`.
- **The second application reads `a` back through the environment** —
  `i64.load` of `[env+4]` — subtracts, masks with `0x7fffffffffffffff`, and
  boxes the result in 8 fresh bytes. The mask is where `Uint63`'s wrapping
  lives.
- **Both loads are `align=4`.** The boxes the adapter hands over need 4-byte
  alignment, not 8; only the canonical-ABI return area needs 8.

One more measurement, which is why this example is `minus` and not `plus2`:
compiling `fun a b => (a + b)%uint63` instead produces a module that differs in
exactly one opcode — `i64.add` where this one has `i64.sub`. Everything above
is the shape of a two-argument export, not something about subtraction.

[`stub/backend.wat`](stub/backend.wat) mirrors all of it, including the two
allocations in the first application, since that is what makes the middle
out-of-memory branch reachable at all.

## The files

| | Who writes it |
| --- | --- |
| [`wit/world.wit`](wit/world.wit) | you — the interface comes first |
| [`wit/deps/certirocq-abi/`](wit/deps/certirocq-abi) | vendored, because resolving the world needs the package on disk beside it |
| [`interface.v`](interface.v) | generated — two WIT parameters as two Rocq arrows |
| [`impl.v`](impl.v) | **you** — the only file that is yours |
| [`extraction.v`](extraction.v) | generated — the `CertiRocq Compile Wasm` call |
| [`adapter.wat`](adapter.wat) | generated — the canonical-ABI glue |
| [`component.wat`](component.wat) | generated — the component shell: the world's type, around the two core modules |
| [`Makefile`](Makefile) | generated — `rocq c`, then splice the backend and the adapter into `component.wat` |
| [`component.toml`](component.toml) | you — what the WIT file cannot say |
| [`stub/`](stub) | neither; it stands in for CertiRocq's output |

## Known gaps

- **Three parameters are not measured.** Two is enough to show the loop repeats,
  and the generator emits it for any *n* — `bindgen/tests/wit/good/arity3.wit`
  is a three-parameter world with the immediate one in the middle, and
  `checks.generated` parses the adapter it produces. But parsing is not
  running: only *n* = 2 has been through a real backend.
- **Every call re-runs `main_function`**, as in `plus1`, and a two-argument
  call now leaks a closure and an environment on top of the result: the fix is
  the [heap reset](../../TODO.md#heap-reset), and the cost is calls rather
  than correctness.
- **One `unreachable`** remains in the adapter, for a memory with no room for
  the 32-byte reservation at the bottom of a fresh heap. A failure with nowhere
  to write cannot be reported through a return area; nothing else here traps.
