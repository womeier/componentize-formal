;; stub/backend.wat with one number changed, so the heap runs out *between*
;; the two applications. `mem_ptr` starts 48 bytes below the page boundary:
;; the 32 bytes the adapter reserves fit, the 12-byte closure
;; `main_function` builds fits exactly, and the first allocation the first
;; application makes does not. The code is a copy with the prose stripped --
;; read stub/backend.wat for what it does.
;;
;; This is the branch neither `plus1`'s nor `record`'s out-of-memory stub can
;; reach: a failure in the *middle* of the argument loop. `main_function`
;; succeeds, the first application fails, and `result` still holds the closure
;; `main_function` left there. An adapter that checked only after the last
;; application would carry on and apply `b` to that stale closure --- here
;; that is the first wrapper again, which merely computes the wrong thing,
;; but in general it is a body that traps on an argument of the wrong shape.
;; See the comment on the middle check in adapter.wat.
(module
  (memory (export "memory") 1)
  (global $mem_ptr (export "mem_ptr") (mut i32) (i32.const 65488))
  (global $result (export "result") (mut i32) (i32.const 0))
  (global $out_of_mem (export "out_of_mem") (mut i32) (i32.const 0))
  (table (export "table") 3 funcref)
  (elem (i32.const 1) $minus_a)
  (elem (i32.const 2) $minus_b)

  ;; Dirty, as in stub/backend.wat, over the bytes this call reaches.
  (data (i32.const 65488)
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
    (i32.store (local.get $closure) (i32.const 0))
    (i32.store offset=4 (local.get $closure) (i32.const 1))
    (i32.store offset=8 (local.get $closure) (i32.const 1))
    (global.set $result (local.get $closure)))

  (func $minus_a (param $env i32) (param $arg i32)
    (local $captured i32)
    (local $closure i32)
    (local.set $captured (call $alloc (i32.const 8)))
    (if (global.get $out_of_mem) (then (return)))
    (i32.store (local.get $captured) (i32.const 0))
    (i32.store offset=4 (local.get $captured) (local.get $arg))
    (local.set $closure (call $alloc (i32.const 12)))
    (if (global.get $out_of_mem) (then (return)))
    (i32.store (local.get $closure) (i32.const 0))
    (i32.store offset=4 (local.get $closure) (i32.const 2))
    (i32.store offset=8 (local.get $closure) (local.get $captured))
    (global.set $result (local.get $closure)))

  (func $minus_b (param $env i32) (param $arg i32)
    (local $a i32)
    (local $out i32)
    (local.set $a (i32.load offset=4 (local.get $env)))
    (local.set $out (call $alloc (i32.const 8)))
    (if (global.get $out_of_mem) (then (return)))
    (i64.store (local.get $out)
      (i64.and (i64.sub (i64.load (local.get $a)) (i64.load (local.get $arg)))
               (i64.const 0x7fffffffffffffff)))
    (global.set $result (local.get $out)))
)
