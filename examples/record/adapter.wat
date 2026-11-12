;; componentize-formal generate wrote this from the world `wrapper-world`:
;;
;;     export grow: func(s: span) -> result<span, error>
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

  ;; result<span, error> in the canonical ABI: size 24, align 8, a 1-byte
  ;; discriminant at offset 0 and the payload at offset 8. `result`
  ;; declares ok before err, so ok = 0 and err = 1.
  (func $ok (param $ret i32) (param $val i32)
    (i32.store8 (local.get $ret) (i32.const 0))
    (i64.store offset=8 (local.get $ret) (i64.load (i32.load offset=4 (local.get $val))))
    (i64.store offset=16 (local.get $ret) (i64.load (i32.load offset=8 (local.get $val)))))

  ;; The error half is an enum, so its payload is a bare discriminant:
  ;; out-of-memory = 0, out-of-range = 1. An enum's case order *is* that
  ;; discriminant, which is why the package declaring it is versioned
  ;; (TODO.md#abi-package-version).
  (func $err (param $ret i32) (param $case i32)
    (i32.store8 (local.get $ret) (i32.const 1))
    (i32.store8 offset=8 (local.get $ret) (local.get $case)))

  ;; The canonical ABI flattens a record into its fields' slots,
  ;; in declaration order -- so nothing arrives by pointer and the
  ;; adapter has to *build* the value rather than box one:
  ;;
  ;; `s: span` arrives in $s_start, $s_byte_length.
  (func (export "grow") (param $s_start i64) (param $s_byte_length i64) (result i32)
    (local $ret i32)
    (local $arg i32)
    (local $closure i32)

    ;; The return area and the boxed argument come off the same bump
    ;; pointer the backend allocates from, before `main_function` runs,
    ;; so the heap is still empty and the reservation cannot fail in
    ;; practice.
    ;;
    ;; 8-aligned because the host rejects a misaligned return area,
    ;; and `mem_ptr` is only 4-aligned in general (repr.rs:
    ;; 4-byte cells); reserving a multiple of 8 leaves it where the
    ;; backend wants it again.
    (global.set $mem_ptr
      (i32.and (i32.add (global.get $mem_ptr) (i32.const 7)) (i32.const -8)))
    (local.set $ret (global.get $mem_ptr))
    (local.set $arg (i32.add (local.get $ret) (i32.const 24)))
    (global.set $mem_ptr (i32.add (local.get $ret) (i32.const 52)))

    ;; Nothing bounds-checks what the adapter takes for itself, and a store
    ;; past the end of the memory would trap instead of answering. Reaching
    ;; here means there is no room for the return area at the bottom of a
    ;; fresh heap, which cannot be reported through a return area that does
    ;; not fit -- so it is the one failure with nowhere to go.
    (if (i32.gt_u (global.get $mem_ptr)
                  (i32.mul (memory.size) (i32.const 65536)))
      (then unreachable))

    ;; `s`'s `start` field as a `u64` of 2^63 or more has no `Uint63.int`, so it
    ;; is rejected before the program runs rather than silently losing
    ;; its top bit. Read as an i64 that is exactly the negative half.
    (if (i64.lt_s (local.get $s_start) (i64.const 0))
      (then
        (call $err (local.get $ret) (i32.const 1))
        (return (local.get $ret))))

    ;; `s`'s `byte-length` field as a `u64` of 2^63 or more has no `Uint63.int`, so it
    ;; is rejected before the program runs rather than silently losing
    ;; its top bit. Read as an i64 that is exactly the negative half.
    (if (i64.lt_s (local.get $s_byte_length) (i64.const 0))
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

    ;; Build the `span`. One constructor, so an ordinary boxed one:
    ;; the ordinal in the first 4-byte cell and one cell per field
    ;; after it (repr.rs BOXED_ORDINAL_OFFSET, boxed_field_offset),
    ;; the ordinal being 0.
    ;;
    ;; It is written rather than assumed: the compiled projections
    ;; load it and trap on anything else, which an adapter tested
    ;; over a zeroed page would never find out.
    ;;
    ;; Each cell holds whatever its field's own representation is --
    ;; a pointer for a boxed one, an immediate for the rest -- so the 8
    ;; byte payloads go down first and the cells point at them.
    (i64.store offset=12 (local.get $arg) (local.get $s_start))
    (i64.store offset=20 (local.get $arg) (local.get $s_byte_length))
    (i32.store (local.get $arg) (i32.const 0))
    (i32.store offset=4 (local.get $arg) (i32.add (local.get $arg) (i32.const 12)))
    (i32.store offset=8 (local.get $arg) (i32.add (local.get $arg) (i32.const 20)))

    ;; Closure layout: [tag, func_idx, env_ptr] in 4-byte cells, so the
    ;; environment is at offset 8 and the index at offset 4.
    ;; repr.rs CLOSURE_ENV_OFFSET, CLOSURE_FUNC_IDX_OFFSET.
    (call_indirect (type $closure)
      (i32.load offset=8 (local.get $closure))
      (local.get $arg)
      (i32.load offset=4 (local.get $closure)))

    ;; The call allocated its own result, so this is the last place
    ;; the flag can have been set.
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))

    ;; `result` holds a pointer to the `span` block, whose cells
    ;; are pointers to the 2 boxed field(s) -- so a field is two
    ;; loads, not one, and `$ok` does them.
    (call $ok (local.get $ret) (global.get $result))
    (local.get $ret))
)
