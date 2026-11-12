;; The component shell for
;;
;;     variant byte-count { unknown, exactly(u64), at-least(u64), too-big }
;;     export add-byte: func(c: byte-count) -> result<byte-count, error>
;;
;; Hand-written in the shape `componentize-formal generate` has to emit, as a
;; target for the variant support the gate still rejects
;; (TODO.md#structured-types).
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

  ;; `variant byte-count { unknown, exactly(u64), at-least(u64), too-big }`:
  ;; declared by the world, so imported as a type -- the component's type
  ;; carries the world's name for it.
  (type $byte-count.def (variant
    (case "unknown")
    (case "exactly" u64)
    (case "at-least" u64)
    (case "too-big")))
  (import "byte-count" (type $byte-count (eq $byte-count.def)))

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

  (type $ret.ty (result $byte-count (error $error)))
  (type $sig.ty (func (param "c" $byte-count) (result $ret.ty)))
  (alias core export $a "add-byte" (core func $add-byte.core))
  (func $add-byte (type $sig.ty) (canon lift (core func $add-byte.core) (memory $memory)))
  (export "add-byte" (func $add-byte))
)
