;; Canonical-ABI glue for
;;
;;     export negb: func(b: bool) -> result<bool, error>
;;
;; Hand-written in the shape `componentize-formal generate` has to emit. The
;; contrast with examples/plus1 is the point: same structure, different layout
;; and one fewer failure mode, both read off the world rather than chosen.
(module
  (import "rocq" "memory" (memory 1))
  (import "rocq" "mem_ptr" (global $mem_ptr (mut i32)))
  (import "rocq" "result" (global $result (mut i32)))
  (import "rocq" "out_of_mem" (global $out_of_mem (mut i32)))
  (import "rocq" "main_function" (func $main_function))
  (import "rocq" "__indirect_function_table" (table $funcs 1 funcref))

  (type $closure (func (param i32 i32)))

  ;; result<bool, error> in the canonical ABI: size 2, align 1 -- a one-byte
  ;; discriminant at offset 0 and a one-byte payload at offset 1. plus1's is
  ;; 16 bytes at align 8 for the same `result` former, because the payload is
  ;; an i64 there. Neither number is a constant of the generator; both come
  ;; out of the type.
  (func $ok (param $ret i32) (param $val i32)
    (i32.store8 (local.get $ret) (i32.const 0))
    (i32.store8 offset=1 (local.get $ret) (local.get $val)))

  (func $err (param $ret i32) (param $case i32)
    (i32.store8 (local.get $ret) (i32.const 1))
    (i32.store8 offset=1 (local.get $ret) (local.get $case)))

  (func (export "negb") (param $b i32) (result i32)
    (local $ret i32)
    (local $closure i32)

    ;; 8-align the start and reserve a multiple of 8, as in plus1. The type
    ;; here needs neither -- align 1, size 2 -- but `mem_ptr` has to stay
    ;; 4-aligned for the backend's cells, and 8 covers every return area
    ;; without case analysis.
    (global.set $mem_ptr
      (i32.and (i32.add (global.get $mem_ptr) (i32.const 7)) (i32.const -8)))
    (local.set $ret (global.get $mem_ptr))
    (global.set $mem_ptr (i32.add (local.get $ret) (i32.const 8)))

    (if (i32.gt_u (global.get $mem_ptr)
                  (i32.mul (memory.size) (i32.const 65536)))
      (then unreachable))

    ;; No range check: every component-model `bool` has a Rocq `bool`, so
    ;; out-of-memory is the only way this call can fail. The world still
    ;; returns `result<bool, error>` -- `main_function` allocates, so there is
    ;; no signature here that cannot fail.
    (call $main_function)
    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))
    (local.set $closure (global.get $result))

    ;; A nullary constructor is an immediate, so the argument is the value
    ;; itself rather than a pointer to it -- the other half of plus1's boxed
    ;; int. `(b << 1) | 1` turns the component's 0/1 into the backend's 1/3.
    ;;
    ;; That shift is this simple only because the CertiRocq pipeline swaps
    ;; `bool`'s constructors to match OCaml's `false = 0, true = 1`; Rocq
    ;; declares `true` first, so declaration order would have given the
    ;; opposite pair and this would need to invert. See repr.rs BOOL_FALSE.
    (call_indirect (type $closure)
      (i32.load offset=8 (local.get $closure))
      (i32.or (i32.shl (local.get $b) (i32.const 1)) (i32.const 1))
      (i32.load offset=4 (local.get $closure)))

    (if (global.get $out_of_mem)
      (then
        (call $err (local.get $ret) (i32.const 0))
        (return (local.get $ret))))

    ;; `result` holds the immediate itself, not a pointer: nothing was
    ;; allocated to return a nullary constructor.
    (call $ok (local.get $ret)
      (i32.shr_u (global.get $result) (i32.const 1)))
    (local.get $ret))
)
