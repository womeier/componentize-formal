;; Stands in for the module CertiRocq emits, so the adapter can be exercised
;; without Rocq and without the nixpkgs pin. It presents the same surface --
;; the five exports in repr.rs plus the table -- and the same value
;; representation: a primitive int boxed as a pointer to 8 bytes, a closure as
;; [tag, func_idx, env_ptr], and a bump allocator that sets `out_of_mem`
;; rather than trapping.
;;
;; What it mirrors that `plus1`'s stub does not is the currying. A compiled
;; `int -> int -> int` is two one-argument functions, not one two-argument
;; function: applying the first argument *allocates a closure* and leaves it
;; in `result`, and only the second application computes anything. Both
;; wrappers below were read off a module the real CertiRocq compiled -- see
;; README.md, "Measured, not inferred".
;;
;; It is not a substitute for running the real thing. What it pins is the
;; adapter's half: the argument loop, the canonical-ABI lowering, and that all
;; three out-of-memory checks fire. `make run` is the real path.
(module
  (memory (export "memory") 1)
  (global $mem_ptr (export "mem_ptr") (mut i32) (i32.const 0))
  (global $result (export "result") (mut i32) (i32.const 0))
  (global $out_of_mem (export "out_of_mem") (mut i32) (i32.const 0))
  (table (export "table") 3 funcref)
  (elem (i32.const 1) $minus_a)
  (elem (i32.const 2) $minus_b)

  ;; The heap does not arrive zeroed in general: a module can carry data
  ;; segments, and `mem_ptr` is never reset between calls
  ;; (TODO.md#heap-reset). Nothing here depends on the adapter writing a cell
  ;; the backend would otherwise find zero -- that is `record`'s ordinal, and
  ;; `minus` has no constructor to build -- but a stub over a fresh page is
  ;; more forgiving than a used heap for no reason, so the bytes this call
  ;; reaches arrive dirty.
  (data (i32.const 0)
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5")

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
  ;; and leaving it in `result`. The env is an immediate rather than a
  ;; pointer: nothing is captured yet, and the first wrapper ignores it, which
  ;; is what the compiled module does too.
  (func (export "main_function")
    (local $closure i32)
    (local.set $closure (call $alloc (i32.const 12)))
    (if (global.get $out_of_mem) (then (return)))
    (i32.store (local.get $closure) (i32.const 0))              ;; tag
    (i32.store offset=4 (local.get $closure) (i32.const 1))     ;; func_idx
    (i32.store offset=8 (local.get $closure) (i32.const 1))     ;; env
    (global.set $result (local.get $closure)))

  ;; The first application: `fun a => fun b => ...`, applied to `a`. Nothing
  ;; is computed. It captures the argument *by address* -- the cell holds the
  ;; adapter's own box -- and hands back a closure for the second wrapper.
  ;; Two allocations, either of which can be the one that fails.
  (func $minus_a (param $env i32) (param $arg i32)
    (local $captured i32)
    (local $closure i32)
    (local.set $captured (call $alloc (i32.const 8)))
    (if (global.get $out_of_mem) (then (return)))
    (i32.store (local.get $captured) (i32.const 0))             ;; tag
    (i32.store offset=4 (local.get $captured) (local.get $arg))  ;; &a
    (local.set $closure (call $alloc (i32.const 12)))
    (if (global.get $out_of_mem) (then (return)))
    (i32.store (local.get $closure) (i32.const 0))              ;; tag
    (i32.store offset=4 (local.get $closure) (i32.const 2))     ;; func_idx
    (i32.store offset=8 (local.get $closure) (local.get $captured))
    (global.set $result (local.get $closure)))

  ;; The second application: `(a - b)%uint63`. `a` comes back out of the
  ;; environment, through the pointer stored there, and the subtraction wraps
  ;; at 2^63 because that is what Uint63 does -- the compiled body masks with
  ;; 0x7fffffffffffffff exactly here.
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
