# ADR 0009: Pin the sqlite-vec fork with connection-local registration

## Status

Accepted. Supersedes ADR 0008's dependency and registration choice; its vector
schema, model identity and `VectorIndex` boundary remain in effect.

## Decision

Depend on `https://github.com/cercova-studios/sqlite-vec` at commit
`98c0e487fb96acef784ad64dceb78fcb11a55edc` from `codex/dev-local-datastore`.
Cargo consumes its committed C amalgamation and typed Rust FFI entrypoint.
The branch name identifies provenance; the full revision controls the build.
Keep rusqlite 0.32 with bundled SQLite as the application's single runtime.
Consumer builds need Cargo and a C compiler, without Make or a Lance runtime.

`vecstore::install(&Connection)` registers on each live catalog connection,
including the connection used to inspect an existing schema marker. Registration
errors return an operational `sqlite_vec` error. Remove the process-global
`Once` and function-pointer transmute; the fork declares the actual C signature.

Both indexes keep flat float32[384] vectors and cosine distance. No schema-version
bump, re-embedding, index conversion, CLI change or ranking change accompanies
this dependency switch.

## Why this is better for agents

A full revision makes the native implementation reproducible. Registration
belongs to the connection that uses it, so it does not rely on process-global
state or change unrelated SQLite connections. The existing vector boundary
continues to contain KNN SQL.

## Compatibility and recovery

The previous lockfile selected registry sqlite-vec 0.1.9. Tests use that exact
release in a separate executable, with bundled rusqlite 0.32.1, to avoid linking
two definitions of the native extension into one process. They exercise the
production cosine schemas with synthetic vectors and temporary databases.
This qualifies the tested schemas and operations, not all fork capabilities.

Keep a SQLite-consistent pre-upgrade catalog backup and the old executable.
Restore them together to roll back. Preserve `state.json` and transcript files
with the backup. A copied live main database without its committed WAL contents
is not a consistent backup.

For an older writer to modify a fork-written catalog while preserving new data,
first stop all other writers and open it under the fork. In one transaction,
delete keys matching `zonemap_*` from `vec_videos_info` and `vec_chunks_info`,
then commit, close the fork connection and switch writers. Missing bounds use
normal filtering. An older writer does not maintain bounds: leaving them behind
can cause later fork queries to omit matching rows. This maintenance is an
exception to the rule against direct shadow-table writes. Qualify recovery on a
disposable copy before applying it to user data. See the pinned fork's
[Rust compatibility contract](https://github.com/cercova-studios/sqlite-vec/blob/98c0e487fb96acef784ad64dceb78fcb11a55edc/bindings/rust/README.md).

## Verification

Fetch dependencies before entering an offline environment:

```sh
cargo fetch --locked
cargo fetch --locked --manifest-path tests/upstream/Cargo.toml
```

The existing embedding dependency also needs its native ONNX Runtime build
artifact available before a fully disconnected first build. Tests do not load
an embedding model. Run `cargo test --offline --test vector_compat` for the
focused suite and the normal formatting, strict Clippy and full test checks.
The compatibility test builds the isolated upstream helper with `--offline
--locked`; missing cached inputs fail the test rather than skipping coverage.
