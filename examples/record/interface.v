(* The shape `componentize-formal generate` will emit from wit/world.wit once
 * records are supported. It cannot emit this today: the gate rejects a
 * `record` with E-STRUCTURED, and this file is what it is being written
 * against (TODO.md#structured-types).
 *
 * A WIT record becomes a Rocq `Record`, which is a one-constructor inductive
 * -- so the representation is an ordinary boxed constructor and the
 * projections come for free. The field names are the WIT field names with `-`
 * turned into `_`.
 *
 * Note what is absent, as in every generated interface: `grow` returns
 * `result<span, error>` in the world and `span -> span` here. Both failures
 * -- out of memory, and a field that no `Uint63.int` represents -- belong to
 * the adapter. *)
From Stdlib Require Import Uint63.

(* record span { start: u64, byte-length: u64 } *)
Record span := mk_span { start : int ; byte_length : int }.

Module Type WrapperWorld.
  (* export grow: func(s: span) -> result<span, error> *)
  Parameter grow : span -> span.
End WrapperWorld.
