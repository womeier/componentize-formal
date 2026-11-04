;; The same surface as stub/backend.wat, with a memory too small for the one
;; closure `main_function` builds -- so `out_of_mem` is set before the adapter
;; ever reaches the call. Exists to exercise the branch that is otherwise
;; unreachable at this size, since a 1-page heap cannot be exhausted by a
;; 12-byte closure and an 8-byte int.
(module
  (memory (export "memory") 1)
  ;; Just below the page boundary: the 24 bytes the adapter reserves fit, the
  ;; 12-byte closure after them does not.
  (global $mem_ptr (export "mem_ptr") (mut i32) (i32.const 65512))
  (global $result (export "result") (mut i32) (i32.const 0))
  (global $out_of_mem (export "out_of_mem") (mut i32) (i32.const 0))
  (table (export "table") 2 funcref)

  (func (export "main_function")
    (global.set $out_of_mem (i32.const 1)))
)
