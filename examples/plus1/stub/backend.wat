;; Stands in for the module CertiRocq emits, so the adapter can be exercised
;; without building the CertiRocq that exports the table
;; (TODO.md#table-export). It presents the same surface -- the five exports in
;; repr.rs plus the table -- and the same value representation: a primitive
;; int boxed as a pointer to 8 bytes, a closure as [tag, func_idx, env_ptr],
;; and a bump allocator that sets `out_of_mem` rather than trapping.
;;
;; It is not a substitute for running the real thing. What it pins is the
;; adapter's half: the canonical-ABI lowering, the discriminants, and that the
;; two checks fire. Layer 2 of representation conformance needs the backend.
(module
  (memory (export "memory") 1)
  (global $mem_ptr (export "mem_ptr") (mut i32) (i32.const 0))
  (global $result (export "result") (mut i32) (i32.const 0))
  (global $out_of_mem (export "out_of_mem") (mut i32) (i32.const 0))
  (table (export "table") 2 funcref)
  (elem (i32.const 1) $plus1_body)

  ;; The allocator: bump, or set the flag. Never traps, never reclaims.
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

  ;; `main_function` runs the program; here that is building the one closure
  ;; and leaving it in `result`.
  (func (export "main_function")
    (local $closure i32)
    (local.set $closure (call $alloc (i32.const 12)))
    (if (global.get $out_of_mem) (then (return)))
    (i32.store (local.get $closure) (i32.const 0))              ;; tag
    (i32.store offset=4 (local.get $closure) (i32.const 1))     ;; func_idx
    (i32.store offset=8 (local.get $closure) (i32.const 0))     ;; env_ptr
    (global.set $result (local.get $closure)))

  ;; `fun n => (n + 1)%uint63`, compiled the way the backend would: the
  ;; argument arrives boxed, the result is allocated and boxed, and the add
  ;; wraps at 2^63 because that is what Uint63 does.
  (func $plus1_body (param $env i32) (param $arg i32)
    (local $out i32)
    (local.set $out (call $alloc (i32.const 8)))
    (if (global.get $out_of_mem) (then (return)))
    (i64.store (local.get $out)
      (i64.and (i64.add (i64.load (local.get $arg)) (i64.const 1))
               (i64.const 0x7fffffffffffffff)))
    (global.set $result (local.get $out)))
)
