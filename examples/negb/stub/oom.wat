;; The same surface with a heap too small for the closure `main_function`
;; builds, so `out_of_mem` is set before the adapter reaches the call. The
;; 8 bytes the adapter reserves fit; the 12-byte closure after them does not.
(module
  (memory (export "memory") 1)
  (global $mem_ptr (export "mem_ptr") (mut i32) (i32.const 65528))
  (global $result (export "result") (mut i32) (i32.const 0))
  (global $out_of_mem (export "out_of_mem") (mut i32) (i32.const 0))
  (table (export "table") 2 funcref)

  (func (export "main_function")
    (global.set $out_of_mem (i32.const 1)))
)
