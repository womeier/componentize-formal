;; Canonical-ABI glue for
;;
;;     record span { start: u64, byte-length: u64 }
;;     export grow: func(s: span) -> result<span, error>
;;
;; Hand-written in the shape `componentize-formal generate` has to emit, as a
;; target for the record support the gate still rejects
;; (TODO.md#structured-types). Everything here is derived from the world and
;; from `bindgen/src/repr.rs`; nothing is specific to `grow` beyond the export
;; name, the two fields and the one result.
;;
;; The record is what makes this different from `plus1`. A WIT record of two
;; `u64`s flattens into two core `i64` parameters, so nothing arrives by
;; pointer -- and the Rocq side wants a boxed constructor with a pointer per
;; field, so the adapter has to *build* a value rather than box one.
(module
  ;; The compiled module. `wasm-merge` resolves these against its exports, so
  ;; none of them survives into the component. Names: repr.rs `exports`.
  (import "rocq" "memory" (memory 1))
  (import "rocq" "mem_ptr" (global $mem_ptr (mut i32)))
  (import "rocq" "result" (global $result (mut i32)))
  (import "rocq" "out_of_mem" (global $out_of_mem (mut i32)))
  (import "rocq" "main_function" (func $main_function))
  (import "rocq" "table" (table $funcs 1 funcref))

  ;; How the backend calls a closure body: (env, arg) with the result left in
  ;; the `result` global. repr.rs `CLOSURE_*`.
  (type $closure (func (param i32 i32)))

  ;; result<span, error> in the canonical ABI: size 24, align 8, the
  ;; discriminant one byte at offset 0 and the payload at offset 8. `span` is
  ;; size 16, align 8, so its fields land at 8 and 16 -- in declaration order,
  ;; which is the only order the component model knows about.
  (func $ok (param $ret i32) (param $start i64) (param $byte_length i64)
    (i32.store8 (local.get $ret) (i32.const 0))
    (i64.store offset=8 (local.get $ret) (local.get $start))
    (i64.store offset=16 (local.get $ret) (local.get $byte_length)))

  ;; `error` is an enum, so its payload is its own discriminant:
  ;; out-of-memory = 0, out-of-range = 1.
  (func $err (param $ret i32) (param $case i32)
    (i32.store8 (local.get $ret) (i32.const 1))
    (i32.store8 offset=8 (local.get $ret) (local.get $case)))

  (func (export "grow") (param $start i64) (param $byte_length i64) (result i32)
    (local $ret i32)
    (local $arg i32)
    (local $closure i32)

    ;; Everything the adapter needs comes off the same bump pointer the
    ;; backend allocates from, before `main_function` runs so that the heap is
    ;; still empty and the reservation cannot fail in practice:
    ;;
    ;;   $ret + 0   .. 24   the canonical-ABI return area
    ;;   $ret + 24  .. 32   `start`, boxed
    ;;   $ret + 32  .. 40   `byte-length`, boxed
    ;;   $arg + 0   .. 12   the `span` itself: [ordinal, &start, &byte-length]
    ;;
    ;; 8-aligned because the host rejects a misaligned return area, and
    ;; `mem_ptr` is only 4-aligned in general (repr.rs: 4-byte cells). The
    ;; two boxes inherit that alignment; the record cells are 4-byte cells and
    ;; want no more.
    (global.set $mem_ptr
      (i32.and (i32.add (global.get $mem_ptr) (i32.const 7)) (i32.const -8)))
    (local.set $ret (global.get $mem_ptr))
    (local.set $arg (i32.add (local.get $ret) (i32.const 40)))
    (global.set $mem_ptr (i32.add (local.get $ret) (i32.const 52)))

    ;; Nothing bounds-checks what the adapter takes for itself, and a store
    ;; past the end of the memory would trap instead of answering. Reaching
    ;; here means the memory has no room for a 52-byte reservation at the
    ;; bottom of a fresh heap, which cannot be reported through a return area
    ;; that does not fit -- so it is the one failure with nowhere to go.
    (if (i32.gt_u (global.get $mem_ptr)
                  (i32.mul (memory.size) (i32.const 65536)))
      (then unreachable))

    ;; Every `u64` field is its own narrowing: 2^63 or more has no
    ;; `Uint63.int`, so it is rejected before the program runs rather than
    ;; silently losing its top bit. One check per field, because a record
    ;; carries no `u64` that is exempt.
    (if (i64.lt_s (local.get $start) (i64.const 0))
      (then
        (call $err (local.get $ret) (i32.const 1))
        (return (local.get $ret))))
    (if (i64.lt_s (local.get $byte_length) (i64.const 0))
      (then
        (call $err (local.get $ret) (i32.const 1))
        (return (local.get $ret))))

    ;; `main_function` runs the whole program and leaves the export closure in
    ;; `result`. It allocates, so it can be what exhausts the memory.
    (call $main_function)
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))
    (local.set $closure (global.get $result))

    ;; Build the `span`. A Rocq record is an ordinary boxed constructor: the
    ;; ordinal in the first 4-byte cell, one cell per field after it
    ;; (repr.rs BOXED_ORDINAL_OFFSET, boxed_field_offset). A record has one
    ;; constructor, so the ordinal is 0 -- and it is written rather than
    ;; assumed, because the compiled projections load it and trap on anything
    ;; else.
    ;;
    ;; Each cell holds a *pointer*: a primitive int is itself boxed, as a
    ;; pointer to its 8-byte payload (repr.rs PRIM_PAYLOAD_SIZE). So the two
    ;; payloads go down first and the record points at them.
    (i64.store offset=24 (local.get $ret) (local.get $start))
    (i64.store offset=32 (local.get $ret) (local.get $byte_length))
    (i32.store (local.get $arg) (i32.const 0))
    (i32.store offset=4 (local.get $arg)
      (i32.add (local.get $ret) (i32.const 24)))
    (i32.store offset=8 (local.get $arg)
      (i32.add (local.get $ret) (i32.const 32)))

    ;; Closure layout: [tag, func_idx, env_ptr] in 4-byte cells.
    ;; repr.rs CLOSURE_FUNC_IDX_OFFSET, CLOSURE_ENV_OFFSET.
    (call_indirect (type $closure)
      (i32.load offset=8 (local.get $closure))
      (local.get $arg)
      (i32.load offset=4 (local.get $closure)))

    ;; The call allocated its own result -- two boxed fields and the record
    ;; around them -- so it is the second place the flag can have been set.
    ;; Checking it is not optional: `glob_out_of_mem` means the value in
    ;; `result` was never written.
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))

    ;; `result` is a pointer to the returned record, whose cells are pointers
    ;; to the boxed fields -- so each field is two loads, not one.
    (call $ok (local.get $ret)
      (i64.load (i32.load offset=4 (global.get $result)))
      (i64.load (i32.load offset=8 (global.get $result))))
    (local.get $ret))
)
