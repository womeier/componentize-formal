;; Canonical-ABI glue for
;;
;;     variant byte-count { unknown, exactly(u64), at-least(u64), too-big }
;;     export add-byte: func(c: byte-count) -> result<byte-count, error>
;;
;; Hand-written in the shape `componentize-formal generate` has to emit, as a
;; target for the variant support the gate still rejects
;; (TODO.md#structured-types). Everything here is derived from the world and
;; from `bindgen/src/repr.rs`; nothing is specific to `add-byte` beyond the
;; export name, the four cases and the one result.
;;
;; This is the example where the two sides disagree about case numbering, and
;; the disagreement is the reason a translation table exists:
;;
;;   WIT discriminant   case             CertiRocq representation
;;   0                  unknown          immediate 1          (unboxed ord 0)
;;   1                  exactly(u64)     [0, &payload]        (boxed ord 0)
;;   2                  at-least(u64)    [1, &payload]        (boxed ord 1)
;;   3                  too-big          immediate 3          (unboxed ord 1)
;;
;; `match_ordinals_with_tag` numbers boxed and unboxed constructors with
;; independent counters (repr.rs `ordinal::assign`), so the two classes reuse
;; the same ordinals and the tag bit is what tells them apart. Every number in
;; that table was read off a module the real CertiRocq compiled -- there is no
;; arithmetic from the discriminant that produces it, which is why the cases
;; below are written out one by one.
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

  ;; result<byte-count, error> in the canonical ABI: size 24, align 8. The
  ;; outer discriminant is one byte at offset 0 and the payload is at offset
  ;; 8, so `byte-count`'s own discriminant lands at 8 and its `u64` at 16.
  ;; `error`, being one byte, also lands at 8.
  (func $ok (param $ret i32) (param $case i32)
    (i32.store8 (local.get $ret) (i32.const 0))
    (i32.store8 offset=8 (local.get $ret) (local.get $case)))

  (func $ok_payload (param $ret i32) (param $case i32) (param $payload i64)
    (call $ok (local.get $ret) (local.get $case))
    (i64.store offset=16 (local.get $ret) (local.get $payload)))

  ;; `error` is an enum, so its payload is its own discriminant:
  ;; out-of-memory = 0, out-of-range = 1.
  (func $err (param $ret i32) (param $case i32)
    (i32.store8 (local.get $ret) (i32.const 1))
    (i32.store8 offset=8 (local.get $ret) (local.get $case)))

  ;; One case of the Rocq value, for a case that carries a payload: a boxed
  ;; constructor `[ordinal, &payload]` in 4-byte cells, with the payload
  ;; itself boxed after it (repr.rs boxed_field_offset, PRIM_PAYLOAD_SIZE).
  ;; `$at` wants 16 bytes, 8-aligned, so the payload is 8-aligned too.
  (func $boxed (param $at i32) (param $ordinal i32) (param $payload i64)
                                                    (result i32)
    (i64.store offset=8 (local.get $at) (local.get $payload))
    (i32.store (local.get $at) (local.get $ordinal))
    (i32.store offset=4 (local.get $at)
      (i32.add (local.get $at) (i32.const 8)))
    (local.get $at))

  ;; A variant flattens to its discriminant plus the join of its payloads, so
  ;; `byte-count` arrives as an i32 and an i64 -- and for `unknown` and
  ;; `too-big` that i64 carries nothing.
  (func (export "add-byte") (param $tag i32) (param $payload i64) (result i32)
    (local $ret i32)
    (local $arg i32)
    (local $closure i32)
    (local $value i32)

    ;; Off the backend's own bump pointer, before `main_function` runs so that
    ;; the heap is still empty and the reservation cannot fail in practice:
    ;;
    ;;   $ret + 0   .. 24   the canonical-ABI return area
    ;;   $arg + 0   .. 8    the constructor: [ordinal, &payload]
    ;;   $arg + 8   .. 16   the payload, boxed
    ;;
    ;; 8-aligned because the host rejects a misaligned return area, and
    ;; `mem_ptr` is only 4-aligned in general (repr.rs: 4-byte cells).
    (global.set $mem_ptr
      (i32.and (i32.add (global.get $mem_ptr) (i32.const 7)) (i32.const -8)))
    (local.set $ret (global.get $mem_ptr))
    (local.set $arg (i32.add (local.get $ret) (i32.const 24)))
    (global.set $mem_ptr (i32.add (local.get $ret) (i32.const 40)))

    ;; Nothing bounds-checks what the adapter takes for itself, and a store
    ;; past the end of the memory would trap instead of answering. Reaching
    ;; here means the memory has no room for a 40-byte reservation at the
    ;; bottom of a fresh heap, which cannot be reported through a return area
    ;; that does not fit -- so it is the one failure with nowhere to go.
    (if (i32.gt_u (global.get $mem_ptr)
                  (i32.mul (memory.size) (i32.const 65536)))
      (then unreachable))

    ;; A discriminant of 4 or more names no case of `byte-count`, so there is
    ;; no Rocq value to build and nothing to report it with. The component
    ;; model permits trapping here, which is the only honest answer.
    (if (i32.ge_u (local.get $tag) (i32.const 4))
      (then unreachable))

    ;; The payload is range-checked where there is a payload. For `unknown`
    ;; and `too-big` that i64 is not part of the value at all, so whatever
    ;; the canonical ABI puts in the slot is none of this adapter's business
    ;; -- and an unconditional check would be relying on what that is.
    (if (i32.and
          (i32.or (i32.eq (local.get $tag) (i32.const 1))
                  (i32.eq (local.get $tag) (i32.const 2)))
          (i64.lt_s (local.get $payload) (i64.const 0)))
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

    ;; Lift: the table above, one discriminant at a time. A nullary
    ;; constructor is the immediate `2*ord+1` (repr.rs nullary_immediate); a
    ;; constructor with a payload is a pointer. The `unreachable` at the end
    ;; is dead, because the range check above has already run -- it is there so
    ;; that a case added to the world and forgotten here cannot fall through
    ;; into whichever branch happens to be last.
    ;;
    ;; The block is unnamed and the branches are `br 0` on purpose: a symbolic
    ;; label would go into the name section, and `wasm-merge` warns about the
    ;; subsection it does not know how to read.
    (block
      (if (i32.eq (local.get $tag) (i32.const 0))          ;; unknown
        (then (local.set $arg (i32.const 1)) (br 1)))
      (if (i32.eq (local.get $tag) (i32.const 1))          ;; exactly(u64)
        (then
          (local.set $arg (call $boxed (local.get $arg) (i32.const 0)
                                       (local.get $payload)))
          (br 1)))
      (if (i32.eq (local.get $tag) (i32.const 2))          ;; at-least(u64)
        (then
          (local.set $arg (call $boxed (local.get $arg) (i32.const 1)
                                       (local.get $payload)))
          (br 1)))
      (if (i32.eq (local.get $tag) (i32.const 3))          ;; too-big
        (then (local.set $arg (i32.const 3)) (br 1)))
      unreachable)

    ;; Closure layout: [tag, func_idx, env_ptr] in 4-byte cells.
    ;; repr.rs CLOSURE_FUNC_IDX_OFFSET, CLOSURE_ENV_OFFSET.
    (call_indirect (type $closure)
      (i32.load offset=8 (local.get $closure))
      (local.get $arg)
      (i32.load offset=4 (local.get $closure)))

    ;; The call allocated its own result, so it is the second place the flag
    ;; can have been set. Checking it is not optional: `glob_out_of_mem` means
    ;; the value in `result` was never written.
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))
    (local.set $value (global.get $result))

    ;; Lower: the same table, read the other way. The tag bit says which class
    ;; the value belongs to, and only then does the number mean anything --
    ;; immediate 3 is `too-big`, while ordinal 1 in a block is `at-least`.
    ;;
    ;; The two `unreachable`s are the backend's own: its compiled `match` on
    ;; this type executes `unreachable` for a value that is neither, and an
    ;; adapter more forgiving than that would turn a representation that had
    ;; drifted into a wrong answer rather than a stop.
    (if (i32.and (local.get $value) (i32.const 1))
      (then
        (if (i32.eq (local.get $value) (i32.const 1))      ;; unboxed ord 0
          (then
            (call $ok (local.get $ret) (i32.const 0))
            (return (local.get $ret))))
        (if (i32.eq (local.get $value) (i32.const 3))      ;; unboxed ord 1
          (then
            (call $ok (local.get $ret) (i32.const 3))
            (return (local.get $ret))))
        unreachable)
      (else
        (if (i32.eq (i32.load (local.get $value)) (i32.const 0))  ;; boxed 0
          (then
            (call $ok_payload (local.get $ret) (i32.const 1)
              (i64.load (i32.load offset=4 (local.get $value))))
            (return (local.get $ret))))
        (if (i32.eq (i32.load (local.get $value)) (i32.const 1))  ;; boxed 1
          (then
            (call $ok_payload (local.get $ret) (i32.const 2)
              (i64.load (i32.load offset=4 (local.get $value))))
            (return (local.get $ret))))
        unreachable))
    (local.get $ret))
)
