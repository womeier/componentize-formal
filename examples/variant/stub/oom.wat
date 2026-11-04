;; stub/backend.wat with one number changed, so the heap runs out in the
;; middle of the call rather than before it. `mem_ptr` starts 56 bytes below
;; the page boundary: the 40 bytes the adapter reserves fit, the 12-byte
;; closure `main_function` builds fits, and the first allocation the body
;; makes does not. The code is a copy with the prose stripped -- read
;; stub/backend.wat for what it does.
;;
;; This is the branch `plus1`'s out-of-memory stub cannot reach. There the
;; flag is already set when `main_function` returns, so the adapter's first
;; check answers. Here `main_function` succeeds and the *second* check is the
;; only thing between a reported failure and a wrong answer: the body never
;; wrote `result`, so what is still in it is the closure from before -- an
;; even pointer whose first cell is the closure tag 0, which the lowering
;; would happily read as `exactly`.
(module
  (memory (export "memory") 1)
  (global $mem_ptr (export "mem_ptr") (mut i32) (i32.const 65480))
  (global $result (export "result") (mut i32) (i32.const 0))
  (global $out_of_mem (export "out_of_mem") (mut i32) (i32.const 0))
  (table (export "__indirect_function_table") 2 funcref)
  (elem (i32.const 1) $add_byte_body)

  ;; Dirty, as in stub/backend.wat, over the bytes this call reaches.
  (data (i32.const 65472)
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5")

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
    (i32.store (local.get $closure) (i32.const 0))              ;; tag
    (i32.store offset=4 (local.get $closure) (i32.const 1))     ;; func_idx
    (i32.store offset=8 (local.get $closure) (i32.const 0))     ;; env_ptr
    (global.set $result (local.get $closure)))

  (func $successor (param $ordinal i32) (param $payload i64) (result i32)
    (local $box i32)
    (local $out i32)
    (local $sum i64)
    (local.set $sum
      (i64.and (i64.add (local.get $payload) (i64.const 1))
               (i64.const 0x7fffffffffffffff)))
    (if (i64.eqz (local.get $sum))
      (then (return (i32.const 3))))                            ;; too_big
    (local.set $box (call $alloc (i32.const 8)))
    (if (global.get $out_of_mem) (then (return (i32.const 0))))
    (i64.store (local.get $box) (local.get $sum))
    (local.set $out (call $alloc (i32.const 8)))
    (if (global.get $out_of_mem) (then (return (i32.const 0))))
    (i32.store (local.get $out) (local.get $ordinal))
    (i32.store offset=4 (local.get $out) (local.get $box))
    (local.get $out))

  (func $at_least_one (result i32)
    (local $box i32)
    (local $out i32)
    (local.set $box (call $alloc (i32.const 8)))
    (if (global.get $out_of_mem) (then (return (i32.const 0))))
    (i64.store (local.get $box) (i64.const 1))
    (local.set $out (call $alloc (i32.const 8)))
    (if (global.get $out_of_mem) (then (return (i32.const 0))))
    (i32.store (local.get $out) (i32.const 1))                  ;; at-least
    (i32.store offset=4 (local.get $out) (local.get $box))
    (local.get $out))

  (func $add_byte_body (param $env i32) (param $arg i32)
    (local $out i32)
    (if (i32.and (local.get $arg) (i32.const 1))
      (then
        (if (i32.eq (local.get $arg) (i32.const 1))             ;; unknown
          (then
            (local.set $out (call $at_least_one))
            (if (global.get $out_of_mem) (then (return)))
            (global.set $result (local.get $out))
            (return)))
        (if (i32.eq (local.get $arg) (i32.const 3))             ;; too_big
          (then
            (global.set $result (i32.const 3))
            (return)))
        unreachable)
      (else
        (if (i32.eq (i32.load (local.get $arg)) (i32.const 0))  ;; exactly
          (then
            (local.set $out (call $successor (i32.const 0)
              (i64.load (i32.load offset=4 (local.get $arg)))))
            (if (global.get $out_of_mem) (then (return)))
            (global.set $result (local.get $out))
            (return)))
        (if (i32.eq (i32.load (local.get $arg)) (i32.const 1))  ;; at-least
          (then
            (local.set $out (call $successor (i32.const 1)
              (i64.load (i32.load offset=4 (local.get $arg)))))
            (if (global.get $out_of_mem) (then (return)))
            (global.set $result (local.get $out))
            (return)))
        unreachable)))
)