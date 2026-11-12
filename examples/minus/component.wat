;; componentize-formal generate wrote this from the world `wrapper-world`:
;;
;;     export minus: func(a: u64, b: u64) -> result<u64, error>
;;
;; The component *is* the link. CertiRocq's module and the adapter go
;; in as two core modules; the adapter's imports are satisfied by
;; instantiating it with the backend's instance. Neither module is
;; rewritten on the way in -- what `rocq c` emitted is what runs.
(component
  ;; The build rules splice `wasm-tools print` of the module `rocq c`
  ;; emits in here, re-headed as `(core module $rocq`. It is the one
  ;; part of this file that does not exist until the Rocq half has
  ;; compiled.
  ;; @BACKEND@

  ;; `use certirocq:abi/errors@0.1.0.{error}`: the enum every export reports
  ;; its failures through, imported as a type so the component's own type
  ;; names the package it came from (TODO.md#abi-package-version). The
  ;; case order *is* the discriminant the adapter stores.
  (type $abi.ty (instance
    (type $e (enum "out-of-memory" "out-of-range"))
    (export "error" (type (eq $e)))))
  (import "certirocq:abi/errors@0.1.0" (instance $abi (type $abi.ty)))
  (alias export $abi "error" (type $error.def))
  (import "error" (type $error (eq $error.def)))

  ;; The adapter, spliced in from adapter.wat -- the canonical-ABI glue,
  ;; re-headed as `(core module $adapter`.
  ;; @ADAPTER@
  (core instance $r (instantiate $rocq))
  (core instance $a (instantiate $adapter (with "rocq" (instance $r))))

  ;; The result is more flat values than the canonical ABI returns, so
  ;; the export hands back a pointer and the host reads the value out of
  ;; the backend's memory -- the same memory the adapter took the return
  ;; area from.
  (alias core export $r "memory" (core memory $memory))

  (type $ret.ty (result u64 (error $error)))
  (type $sig.ty (func (param "a" u64) (param "b" u64) (result $ret.ty)))
  (alias core export $a "minus" (core func $minus.core))
  (func $minus (type $sig.ty) (canon lift (core func $minus.core) (memory $memory)))
  (export "minus" (func $minus))
)
