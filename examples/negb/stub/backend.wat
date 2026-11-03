;; Stands in for the module CertiRocq emits for `negb`, with the same exports
;; and the same representation. See examples/plus1/stub/backend.wat.
(module
  (memory (export "memory") 1)
  (global $mem_ptr (export "mem_ptr") (mut i32) (i32.const 0))
  (global $result (export "result") (mut i32) (i32.const 0))
  (global $out_of_mem (export "out_of_mem") (mut i32) (i32.const 0))
  (table (export "__indirect_function_table") 2 funcref)
  (elem (i32.const 1) $negb_body)

  (func $alloc (param $size i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $mem_ptr))
    (if (i32.gt_u (i32.add (local.get $at) (local.get $size))
                  (i32.mul (memory.size) (i32.const 65536)))
      (then
        (global.set $out_of_mem (i32.const 1))
        (return (i32.const 0))))
    (global.set $mem_ptr (i32.add (local.get $at) (local.get $size)))
    (local.get $at))

  (func (export "main_function")
    (local $closure i32)
    (local.set $closure (call $alloc (i32.const 12)))
    (if (global.get $out_of_mem) (then (return)))
    (i32.store (local.get $closure) (i32.const 0))
    (i32.store offset=4 (local.get $closure) (i32.const 1))
    (i32.store offset=8 (local.get $closure) (i32.const 0))
    (global.set $result (local.get $closure)))

  ;; `fun b => if b then false else true`, on immediates. 1 is `false` and 3
  ;; is `true` -- repr.rs BOOL_FALSE and BOOL_TRUE, which are swapped relative
  ;; to Rocq's declaration order and pinned against the real backend by
  ;; checks/representation.py. Nothing is allocated: a nullary constructor is
  ;; the value.
  (func $negb_body (param $env i32) (param $arg i32)
    (global.set $result
      (if (result i32) (i32.eq (local.get $arg) (i32.const 1))
        (then (i32.const 3))
        (else (i32.const 1)))))
)
