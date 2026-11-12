;; componentize-formal generate wrote this from the world `wrapper-world`:
;;
;;     export minus: func(a: u64, b: u64) -> result<u64, error>
;;
;; It bridges two value representations. CertiRocq's: a bump allocator,
;; 4-byte cells, nullary constructors as immediates. The canonical
;; ABI's: a return area with a discriminant and a payload at fixed
;; offsets. Nothing here is chosen -- the offsets come from
;; bindgen/src/repr.rs and from the same layout code `wasm-tools`
;; validates the component with, so the two cannot disagree about
;; where a byte goes.
(module
  ;; The compiled module. The component instantiates the adapter with
  ;; the backend's instance, so every one of these resolves against its
  ;; exports and none is an import of the component itself. Names:
  ;; repr.rs `exports`.
  (import "rocq" "memory" (memory 1))
  (import "rocq" "mem_ptr" (global $mem_ptr (mut i32)))
  (import "rocq" "result" (global $result (mut i32)))
  (import "rocq" "out_of_mem" (global $out_of_mem (mut i32)))
  (import "rocq" "main_function" (func $main_function))
  (import "rocq" "table" (table $funcs 1 funcref))

  ;; How the backend calls a closure body: (env, arg), with the result
  ;; left in `result`. Every Rocq function has type [i32; n] -> [] and
  ;; the backend builds those types in order, so this is index 2 --
  ;; repr.rs func_type_index.
  (type $closure (func (param i32 i32)))

  ;; result<u64, error> in the canonical ABI: size 16, align 8, a 1-byte
  ;; discriminant at offset 0 and the payload at offset 8. `result`
  ;; declares ok before err, so ok = 0 and err = 1.
  (func $ok (param $ret i32) (param $val i64)
    (i32.store8 (local.get $ret) (i32.const 0))
    (i64.store offset=8 (local.get $ret) (local.get $val)))

  ;; The error half is an enum, so its payload is a bare discriminant:
  ;; out-of-memory = 0, out-of-range = 1. An enum's case order *is* that
  ;; discriminant, which is why the package declaring it is versioned
  ;; (TODO.md#abi-package-version).
  (func $err (param $ret i32) (param $case i32)
    (i32.store8 (local.get $ret) (i32.const 1))
    (i32.store8 offset=8 (local.get $ret) (local.get $case)))

  (func (export "minus") (param $a i64) (param $b i64) (result i32)
    (local $ret i32)
    (local $arg_a i32)
    (local $arg_b i32)
    (local $closure i32)

    ;; The return area and the 2 boxed arguments come off the same bump
    ;; pointer the backend allocates from, before `main_function` runs,
    ;; so the heap is still empty and the reservation cannot fail in
    ;; practice.
    ;;
    ;; Every one is reserved here and none is reused: applying an
    ;; argument captures its cells by address, and a later application
    ;; reads back through that pointer (TODO.md#arity).
    ;;
    ;; 8-aligned because the host rejects a misaligned return area,
    ;; and `mem_ptr` is only 4-aligned in general (repr.rs:
    ;; 4-byte cells); reserving a multiple of 8 leaves it where the
    ;; backend wants it again.
    (global.set $mem_ptr
      (i32.and (i32.add (global.get $mem_ptr) (i32.const 7)) (i32.const -8)))
    (local.set $ret (global.get $mem_ptr))
    (local.set $arg_a (i32.add (local.get $ret) (i32.const 16)))
    (local.set $arg_b (i32.add (local.get $ret) (i32.const 24)))
    (global.set $mem_ptr (i32.add (local.get $ret) (i32.const 32)))

    ;; Nothing bounds-checks what the adapter takes for itself, and a store
    ;; past the end of the memory would trap instead of answering. Reaching
    ;; here means there is no room for the return area at the bottom of a
    ;; fresh heap, which cannot be reported through a return area that does
    ;; not fit -- so it is the one failure with nowhere to go.
    (if (i32.gt_u (global.get $mem_ptr)
                  (i32.mul (memory.size) (i32.const 65536)))
      (then unreachable))

    ;; `a` as a `u64` of 2^63 or more has no `Uint63.int`, so it
    ;; is rejected before the program runs rather than silently losing
    ;; its top bit. Read as an i64 that is exactly the negative half.
    (if (i64.lt_s (local.get $a) (i64.const 0))
      (then
        (call $err (local.get $ret) (i32.const 1))
        (return (local.get $ret))))

    ;; `b` as a `u64` of 2^63 or more has no `Uint63.int`, so it
    ;; is rejected before the program runs rather than silently losing
    ;; its top bit. Read as an i64 that is exactly the negative half.
    (if (i64.lt_s (local.get $b) (i64.const 0))
      (then
        (call $err (local.get $ret) (i32.const 1))
        (return (local.get $ret))))

    ;; `main_function` runs the whole program and leaves the export
    ;; closure in `result`. It allocates, so it can be what
    ;; exhausts the memory.
    (call $main_function)
    ;; `out_of_mem` means the value in `result` was never written, so
    ;; this check is not optional.
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))
    (local.set $closure (global.get $result))

    ;; Argument 1 of 2, `a`.
    ;; A primitive int is boxed: a pointer to its 8-byte payload,
    ;; which is what gets passed, not the value. repr.rs
    ;; PRIM_PAYLOAD_SIZE.
    (i64.store (local.get $arg_a) (local.get $a))

    ;; Closure layout: [tag, func_idx, env_ptr] in 4-byte cells, so the
    ;; environment is at offset 8 and the index at offset 4.
    ;; repr.rs CLOSURE_ENV_OFFSET, CLOSURE_FUNC_IDX_OFFSET.
    (call_indirect (type $closure)
      (i32.load offset=8 (local.get $closure))
      (local.get $arg_a)
      (i32.load offset=4 (local.get $closure)))

    ;; Applying one argument of several allocates -- an environment
    ;; and the next closure -- so the flag can be set here with `result`
    ;; still holding the closure from before. Checking now is what
    ;; stops the next application calling through that stale
    ;; closure, which expects an argument of the shape this one
    ;; took: a wrong answer here, a trap once a body matches on an
    ;; ordinal (TODO.md#structured-types).
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))
    (local.set $closure (global.get $result))

    ;; Argument 2 of 2, `b`.
    ;; A primitive int is boxed: a pointer to its 8-byte payload,
    ;; which is what gets passed, not the value. repr.rs
    ;; PRIM_PAYLOAD_SIZE.
    (i64.store (local.get $arg_b) (local.get $b))

    ;; Application 2 of 2, through the closure the one before
    ;; it left in `result`. The same code as the first: a
    ;; partial application is an ordinary closure.
    (call_indirect (type $closure)
      (i32.load offset=8 (local.get $closure))
      (local.get $arg_b)
      (i32.load offset=4 (local.get $closure)))

    ;; The call allocated its own result, so this is the last place
    ;; the flag can have been set.
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))

    ;; `result` holds a pointer to the 8-byte payload, not
    ;; the value itself.
    (call $ok (local.get $ret) (i64.load (global.get $result)))
    (local.get $ret))
)
