# `negb`

```wit
export negb: func(b: bool) -> result<bool, error>;
```

The same pipeline as [`../plus1`](../plus1) — see that README for what each
file is and who writes it. This one exists for the contrasts, all of which the
generator has to derive rather than choose:

```console
$ nix develop
$ cd examples/negb && make run-stub
negb(true) = ok(false)
negb(false) = ok(true)
$ make run-stub-oom
negb(true) with the heap exhausted = err(out-of-memory)
```

| | `plus1` | `negb` |
| --- | --- | --- |
| return area | 16 bytes, align 8 | **2 bytes, align 1** |
| the argument | a pointer to a boxed i64 | **the immediate itself** |
| failure modes | out-of-memory, out-of-range | **out-of-memory only** |
| still `result<T, error>` | yes | **yes** |

The last row is the one worth dwelling on. Nothing a caller passes can be
out of range — every component-model `bool` has a Rocq `bool` — so the only
way this call fails is the allocator. That is enough: `main_function` runs the
whole program and allocates the closure, so there is no export signature in
this backend that cannot fail, and `option<bool>` would not be able to say
why.

## The one thing this example cannot check

`bool` is the only inductive whose constructors the CertiRocq pipeline
reorders: it swaps them so the pair lines up with OCaml's `false = 0`,
`true = 1`, even though Rocq declares `true` first. That is why the adapter's
lift is the plain `(b << 1) | 1` and the lower is `result >> 1`.

`negb` cannot detect it if that ever changes. It is an involution, so lifting
and lowering with the same wrong mapping cancels out and the answers still
look right — which is exactly why the encoding is pinned by compiling constant
functions instead: see `checks/rocq/constructors.v` and
[`TODO.md#representation`](../../TODO.md#representation).
