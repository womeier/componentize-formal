;; Canonical-ABI glue for
;;
;;     export minus: func(a: u64, b: u64) -> result<u64, error>
;;
;; Hand-written in the shape `componentize-formal generate` has to emit, so
;; there is something known to work to generate against. Everything here is
;; derived from the world and from `bindgen/src/repr.rs`; nothing is specific
;; to `minus` beyond the export name, the two parameters and the one result.
;;
;; What is new over `plus1` is the second parameter, and all of it is the
;; argument loop: one box and one `call_indirect` per argument, with the
;; closure for the next application read out of `result` in between. See
;; TODO.md#arity.
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
  ;;
  ;; One argument, for a two-argument export too: the backend curries, so
  ;; there is no 2-ary core function to call. Measured, not assumed --- every
  ;; function in the table of a compiled `int -> int -> int` has this type.
  (type $closure (func (param i32 i32)))

  ;; result<u64, error> in the canonical ABI: size 16, align 8, the
  ;; discriminant one byte at offset 0 and the payload at offset 8. The case
  ;; order in world.wit is the discriminant, which is why the error enum is
  ;; versioned (TODO.md#abi-package-version).
  (func $ok (param $ret i32) (param $val i64)
    (i32.store8 (local.get $ret) (i32.const 0))
    (i64.store offset=8 (local.get $ret) (local.get $val)))

  ;; `error` is an enum, so its payload is its own discriminant:
  ;; out-of-memory = 0, out-of-range = 1.
  (func $err (param $ret i32) (param $case i32)
    (i32.store8 (local.get $ret) (i32.const 1))
    (i32.store8 offset=8 (local.get $ret) (local.get $case)))

  (func (export "minus") (param $a i64) (param $b i64) (result i32)
    (local $ret i32)
    (local $arg_a i32)
    (local $arg_b i32)
    (local $closure i32)

    ;; The return area and one box per argument come off the same bump pointer
    ;; the backend allocates from, before `main_function` runs so that the
    ;; heap is still empty and the reservation cannot fail in practice.
    ;;
    ;; All of it is reserved up front, and none of it is reused: the closure
    ;; the first application returns captures `$arg_a` by address, so the box
    ;; has to outlive the call that reads it, which is the *second* one.
    ;;
    ;; 8-aligned because the host rejects a misaligned return area, and
    ;; `mem_ptr` is only 4-aligned in general (repr.rs: 4-byte cells).
    (global.set $mem_ptr
      (i32.and (i32.add (global.get $mem_ptr) (i32.const 7)) (i32.const -8)))
    (local.set $ret (global.get $mem_ptr))
    (local.set $arg_a (i32.add (local.get $ret) (i32.const 16)))
    (local.set $arg_b (i32.add (local.get $ret) (i32.const 24)))
    (global.set $mem_ptr (i32.add (local.get $ret) (i32.const 32)))

    ;; Nothing bounds-checks what the adapter takes for itself, and a store
    ;; past the end of the memory would trap instead of answering. Reaching
    ;; here means the memory has no room for a 32-byte return area at the
    ;; bottom of a fresh heap, which cannot be reported through a return area
    ;; that does not fit -- so it is the one failure with nowhere to go.
    (if (i32.gt_u (global.get $mem_ptr)
                  (i32.mul (memory.size) (i32.const 65536)))
      (then unreachable))

    ;; A `u64` of 2^63 or more has no `Uint63.int`, so it is rejected before
    ;; the program runs rather than silently losing its top bit. Every `u64`
    ;; parameter is its own check: one of them being in range says nothing
    ;; about the other.
    (if (i64.lt_s (local.get $a) (i64.const 0))
      (then
        (call $err (local.get $ret) (i32.const 1))
        (return (local.get $ret))))
    (if (i64.lt_s (local.get $b) (i64.const 0))
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

    ;; Argument 1 of 2. A primitive int is boxed: a pointer to its 8-byte
    ;; payload. repr.rs PRIM_PAYLOAD_SIZE.
    (i64.store (local.get $arg_a) (local.get $a))

    ;; Closure layout: [tag, func_idx, env_ptr] in 4-byte cells.
    ;; repr.rs CLOSURE_FUNC_IDX_OFFSET, CLOSURE_ENV_OFFSET.
    (call_indirect (type $closure)
      (i32.load offset=8 (local.get $closure))
      (local.get $arg_a)
      (i32.load offset=4 (local.get $closure)))

    ;; Applying one argument of two allocates: an environment capturing `a`,
    ;; and the closure for the next application. So this is a place the flag
    ;; can be set, with nothing in `result` but the closure that was already
    ;; there.
    ;;
    ;; What this check buys is *not* that the failure would otherwise go
    ;; unreported: `out_of_mem` is only ever set, never cleared, so the check
    ;; after the last application would see it too. It is that without this
    ;; check the adapter goes on to `call_indirect` through a stale closure --
    ;; one expecting an argument of the type the *previous* application took.
    ;; Here both take a boxed int and it would merely compute the wrong thing;
    ;; in general the body loads an ordinal and executes `unreachable` on
    ;; anything else (TODO.md#structured-types), which turns a reportable
    ;; out-of-memory into a trap. So the check belongs after every
    ;; application, and the adapter does not lean on the flag being sticky.
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))

    ;; The partial application is an ordinary closure, read out of `result`
    ;; exactly as the one `main_function` left. This is the step that repeats,
    ;; once per argument, for any arity.
    (local.set $closure (global.get $result))

    ;; Argument 2 of 2.
    (i64.store (local.get $arg_b) (local.get $b))

    (call_indirect (type $closure)
      (i32.load offset=8 (local.get $closure))
      (local.get $arg_b)
      (i32.load offset=4 (local.get $closure)))

    ;; The call allocated its own result, so it is the last place the flag can
    ;; have been set. Checking it is not optional: `glob_out_of_mem` means the
    ;; value in `result` was never written.
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))

    (call $ok (local.get $ret) (i64.load (global.get $result)))
    (local.get $ret))
)
