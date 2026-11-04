;; Stands in for the module CertiRocq emits, so the adapter can be exercised
;; without building the CertiRocq that exports the table
;; (TODO.md#table-export). It presents the same surface -- the five exports in
;; repr.rs plus the table -- and the same value representation: a primitive
;; int boxed as a pointer to 8 bytes, a record as a boxed constructor with one
;; 4-byte cell per field, a closure as [tag, func_idx, env_ptr], and a bump
;; allocator that sets `out_of_mem` rather than trapping.
;;
;; Checked against the real thing rather than imagined: `rocq c extraction.v`
;; compiles today, and the module it produces builds its closure as [0, 3, 0],
;; reads each field through a projection that traps unless the pointer is even
;; and the ordinal is 0, reuses the caller's box for the field it does not
;; change, and masks the sum to 63 bits. All of that is mirrored below.
;;
;; It is not a substitute for running the real thing. What it pins is the
;; adapter's half: the canonical-ABI lowering, the record it builds, the
;; discriminants, and that all three checks fire. Layer 2 of representation
;; conformance needs the backend.
(module
  (memory (export "memory") 1)
  (global $mem_ptr (export "mem_ptr") (mut i32) (i32.const 0))
  (global $result (export "result") (mut i32) (i32.const 0))
  (global $out_of_mem (export "out_of_mem") (mut i32) (i32.const 0))
  (table (export "table") 2 funcref)
  (elem (i32.const 1) $grow_body)

  ;; The allocator: bump, or set the flag. Never traps, never reclaims.


  ;; The heap does not arrive zeroed in general: a module can carry data
  ;; segments, `mem_ptr` is never reset between calls, and nothing in the
  ;; backend's invariants promises a fresh page. So this one arrives dirty,
  ;; over the bytes the one call reaches.
  ;;
  ;; That is what makes the ordinal the adapter writes observable. Against a
  ;; zeroed page an adapter that never wrote it would answer correctly anyway
  ;; -- the cell it forgot already held the value it needed -- and the record
  ;; would only come apart later, on a heap that had been used.
  (data (i32.const 0)
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
    "\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5\a5"
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

  ;; One field read, the way the compiled projections do it. A record is a
  ;; pointer -- so even, since a nullary constructor would be an odd immediate
  ;; -- to a block whose first cell is the ordinal. Anything else is a term
  ;; the Rocq type discipline rules out, and the backend traps on it rather
  ;; than reading on. Kept here so that an adapter which forgot to write the
  ;; ordinal fails loudly instead of being carried by a stub that is more
  ;; forgiving than the thing it stands in for.
  (func $field (param $record i32) (param $index i32) (result i32)
    (if (i32.and (local.get $record) (i32.const 1)) (then unreachable))
    (if (i32.ne (i32.load (local.get $record)) (i32.const 0)) (then unreachable))
    (i32.load (i32.add (local.get $record)
                       (i32.mul (i32.const 4) (i32.add (local.get $index)
                                                       (i32.const 1))))))

  ;; `fun s => mk_span (start s) (byte_length s + 1)%uint63`, compiled the way
  ;; the backend would: both fields read through the projections, the one that
  ;; changes allocated and boxed afresh, the one that does not reusing the
  ;; caller's own box, and the add wrapping at 2^63 because that is what
  ;; Uint63 does.
  (func $grow_body (param $env i32) (param $arg i32)
    (local $start i32)
    (local $length i32)
    (local $out i32)
    (local.set $start (call $field (local.get $arg) (i32.const 0)))
    (local.set $length (call $alloc (i32.const 8)))
    (if (global.get $out_of_mem) (then (return)))
    (i64.store (local.get $length)
      (i64.and (i64.add (i64.load (call $field (local.get $arg) (i32.const 1)))
                        (i64.const 1))
               (i64.const 0x7fffffffffffffff)))
    (local.set $out (call $alloc (i32.const 12)))
    (if (global.get $out_of_mem) (then (return)))
    (i32.store (local.get $out) (i32.const 0))                  ;; ordinal
    (i32.store offset=4 (local.get $out) (local.get $start))
    (i32.store offset=8 (local.get $out) (local.get $length))
    (global.set $result (local.get $out)))
)
