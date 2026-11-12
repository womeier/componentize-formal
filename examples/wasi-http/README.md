# `wasi-http`

The WIT for a `wasi:http` 0.2.8 proxy service, on disk so the target is here
when the support work in
[`TODO.md#generated-examples`](../../TODO.md#generated-examples) lands.

This is **not a working example**: there is no `impl.v` and no build. What
there is, is the world, the dependencies it resolves against, and two checks
that hold both to their word.

## Where the files come from

`proxy.wit`, `types.wit` and `handler.wit` are
[`WebAssembly/wasi-http`](https://github.com/WebAssembly/wasi-http) at `v0.2.8`,
byte for byte.

Everything under [`wit/deps`](wit/deps) is **fetched, not copied**:

```console
$ nix develop
$ make fetch     # restores exactly what wkg.lock pins
$ make update    # moves the pins, and rewrites the lock
```

`wkg` resolves the `@0.2.8` versions named in `proxy.wit`, so the dependency
set is derived from the world rather than chosen by hand, and
[`wkg.lock`](wkg.lock) records a sha256 digest for each package it took. `make
clean && make fetch` reproduces `wit/deps` byte for byte from that lock.

Deliberately *not* trimmed, though the `proxy` world uses neither: `wasi:cli`
arrives whole, and `wasi:filesystem` and `wasi:sockets` arrive because
`wasi:cli`'s `imports` world `include`s them. Trimming is the step that cannot
be checked -- it takes a hand-written package declaration to replace the one
in the deleted files, and from then on nothing on disk can be traced to
anything upstream. The cost of keeping them is 24K.

## What the gate says about it

`componentize-formal generate` writes nothing for this world. The report, in
the order it comes out:

| code | count | what it is |
| --- | --- | --- |
| `E-IMPORT` | 11 | every import the proxy world has, from `wasi:io/poll` to `wasi:http/outgoing-handler` ([`TODO.md#imports`](../../TODO.md#imports)) |
| `E-EXPORT-INTERFACE` | 1 | `wasi:http/incoming-handler` is a namespaced interface export, not `export name: func(..)` ([`TODO.md#exported-interfaces`](../../TODO.md#exported-interfaces)) |
| `E-NO-EXPORT` | 1 | with that export refused the world exports no function, so there would be nothing for CertiRocq to compile ([`TODO.md#nothing-to-compile`](../../TODO.md#nothing-to-compile)) |

The gate stops at the world's shape and never descends into the resource and
`list`/`string` types in `types.wit`, so none of the codes for those appear
here -- they are what the *next* refusal would be, not this one.

## What checks it

* [`checks/generated.py`](../../checks/generated.py) runs the generator and
  requires the refusal above, and that nothing was written. It is in
  `AHEAD_OF_THE_GATE`, so the day the world is supported this check fails and
  says so.
* [`checks/smoke.py`](../../checks/smoke.py) runs `make check-wit`, which
  resolves `wit/` with `wasm-tools` and prints the package and version of
  every package the resolution pulled in. A dependency moving underneath the
  lock changes a line there.

Neither needs a network. `make fetch` and `make update` do, so no check runs
them.
