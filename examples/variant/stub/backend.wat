;; Stands in for the module CertiRocq emits, so the adapter can be exercised
;; without building the CertiRocq that exports the table
;; (TODO.md#table-export). It presents the same surface -- the five exports in
;; repr.rs plus the table -- and the same value representation: a primitive
;; int boxed as a pointer to 8 bytes, a nullary constructor as the immediate
;; `2*ord+1`, a constructor with arguments as a pointer to [ord, ...], a
;; closure as [tag, func_idx, env_ptr], and a bump allocator that sets
;; `out_of_mem` rather than trapping.
;;
;; Checked against the real thing rather than imagined. `rocq c extraction.v`
;; compiles today, and the module it produces says:
;;
;;   unknown        i32.const 1        exactly(n)   [0, &n]
;;   too-big        i32.const 3        at-least(n)  [1, &n]
;;
;; -- boxed and unboxed constructors numbered by independent counters, so the
;; two classes reuse ordinals 0 and 1 and the tag bit is what separates them.
;; Its `match` dispatches on that bit, compares the ordinal or the immediate,
;; and executes `unreachable` on anything else. All of that is mirrored below.
;;
;; It is not a substitute for running the real thing. What it pins is the
;; adapter's half: the lift, the lower, the discriminants, and that all three
;; checks fire. Layer 2 of representation conformance needs the backend.
(module
  (memory (export "memory") 1)
  (global $mem_ptr (export "mem_ptr") (mut i32) (i32.const 0))
  (global $result (export "result") (mut i32) (i32.const 0))
  (global $out_of_mem (export "out_of_mem") (mut i32) (i32.const 0))
  (table (export "table") 2 funcref)
  (elem (i32.const 1) $add_byte_body)

  ;; The heap does not arrive zeroed in general: a module can carry data
  ;; segments, `mem_ptr` is never reset between calls, and nothing in the
  ;; backend's invariants promises a fresh page. So this one arrives dirty,
  ;; over the bytes the one call reaches.
  ;;
  ;; That is what makes the ordinal the adapter writes observable. Against a
  ;; zeroed page an adapter that never wrote it would be read as ordinal 0 --
  ;; `exactly` -- and `exactly` is the case most of these calls want anyway.
  (data (i32.const 0)
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
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
  ;; and leaving it in `result`.
  (func (export "main_function")
    (local $closure i32)
    (local.set $closure (call $alloc (i32.const 12)))
    (if (global.get $out_of_mem) (then (return)))
    (i32.store (local.get $closure) (i32.const 0))              ;; tag
    (i32.store offset=4 (local.get $closure) (i32.const 1))     ;; func_idx
    (i32.store offset=8 (local.get $closure) (i32.const 0))     ;; env_ptr
    (global.set $result (local.get $closure)))

  ;; `ordinal (payload + 1)`, boxed, or `too_big` if the successor wrapped --
  ;; the shape both `exactly` and `at-least` take, differing only in the
  ;; ordinal they keep. The literal boxes the real body allocates for `1` and
  ;; `0` are left out; what matters here is the layout, not the count.
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

  ;; `at_least 1`: the literal has to be boxed and the constructor built
  ;; around it, which is what `unknown` costs on the way out.
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

  ;; `add_byte`, dispatching the way the compiled `match` does: on the tag bit
  ;; first, then on the immediate or the ordinal, with `unreachable` for a
  ;; value that is neither.
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
