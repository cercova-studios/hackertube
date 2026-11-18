# ADR 0010: Reconcile only changed blocked flags

## Status

Accepted.

## Decision

`db::sync_blocked` and `SqliteVec::set_blocked` bind the desired video IDs as
one JSON array. Each table computes membership through SQLite's `json_each`
and updates a row only when its stored flag differs. There is one UPDATE per
table, without a reset pass followed by one scan per marked video.

`state.json` remains authoritative. No fingerprint or persistent desired-set
cache is introduced, so re-ingested vectors, new chunks and expired snoozes
are reconciled even when the supplied mark IDs have not changed. Duplicate
and unknown IDs retain set semantics. Each statement uses one bound JSON
parameter regardless of mark count. The serialized array grows with the set;
quoted IDs remain parameter data rather than SQL text.

## Why this is better for agents

Repeated reads stop rewriting unchanged metadata. The implementation retains
the existing method signatures, tables, state format, eligibility and output.
Tests require zero database changes for an unchanged set and validate new-row
repair, expiry, forget and caller transaction rollback. The SQLite build already
includes JSON functions; no dependency or schema migration is needed.

Each statement keeps SQLite atomicity, and outer transactions still control
rollback. This change does not add cross-table atomicity: a failed call can
leave earlier successful statements committed, as before. Errors propagate and
the next successful reconciliation repairs the derived flags.
