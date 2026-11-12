# ADR 0009: Keep upstream sqlite-vec for now

## Status

Accepted

## Decision

`vecstore::SqliteVec` stays on the crates.io crate `sqlite-vec` 0.1.9 (the
lockfile pin). It is not switched to
[github.com/cercova-studios/sqlite-vec](https://github.com/cercova-studios/sqlite-vec)
commit `af718775a9bdd649fe75666a483e22eac3d42854` (`codex/dev-local-datastore`).
`VectorIndex` remains the swap point, so a later drop-in does not rewrite
search or ingest.

## What that commit changes versus upstream

Upstream here is `asg017/sqlite-vec` `main` at `04d28bd` (`v0.1.10-alpha.4`).
The fork's tree at `af71877` adds:

- A project-local bundle (`scripts/local_bundle.py`, `docs/local-bundle.md`):
  SQLite CLI, static library, read-only search CLI/MCP adapter, and a manifest
  of input hashes. Verified on macOS arm64. Other platforms are called out as
  open.
- Per-connection `sqlite3_vec_init(db, err, api)` in the Rust binding, because
  Apple's SQLite rejects `sqlite3_auto_extension`. Global registration is still
  tested on hosts that allow it.
- `vec0` extras in the C core: optional `indexed by hnsw()`, metadata zone maps,
  adaptive flat reads that skip vector payloads a metadata filter already
  rejected, and a storage-provider hook used by the experimental `lance0` build.
- More modules: `sqlite-vec-hnsw.c`, `sqlite-vec-graph.c` (`graph_witness`),
  `sqlite-vec-sparse.c`, plus the existing IVF and DiskANN files.
- An isolated Rust task-writer prototype (Session capture, undo, claim). Public
  writes stay disabled. It pins rusqlite 0.40.2 because 0.41's Session build
  (`bindgen ^0.73`) does not resolve. It is not linked into the C extension.
- `lance0` blob storage. DROP/identity/schema repairs are recorded. Collation
  and a flaky adapter cancellation test are still open. Public Lance writes
  stay disabled.

`TODO.md` at that commit still has plain-text query embedding (P2) and exposing
HNSW/IVF through the adapter (P3) unchecked. The adapter admits only flat L2
tables.

## Why it is not usable from this CLI yet

A plain clone does not `cargo build`:

- `bindings/rust/Cargo.toml` is `Cargo.toml.tmpl`. `envsubst` and `VERSION`
  generate the real manifest. `cargo add --git` cannot see a package.
- `bindings/rust/sqlite-vec.c` and `sqlite-vec.h` are produced by
  `scripts/amalgamate.py` and the header template. They are not in the tree.
- `vendor/sqlite3.h` is not in the clone. `build.rs` compiles with
  `-DSQLITE_CORE`, which includes `sqlite3.h`. The amalgamation does compile
  on this machine when that header is borrowed from the crates.io 0.1.9 crate
  and the fork's generated header is on the include path first. That is a
  local experiment, not a pinned dependency.

Adopting it would also need:

1. A committed Cargo package whose `cargo build` needs no Make step: generated
   amalgamation, header, and the SQLite headers it was compiled against, at a
   commit.
2. The same SQLite as rusqlite's bundled library. The fork's own note says a
   connection handle from another SQLite is invalid.
3. A check that `vec0` files written by sqlite-vec 0.1.9 still open. The fork
   docs say data-format compatibility has to be checked before a different
   build writes an existing database. Zone-map rows are new.
4. A measurement that HNSW or zone maps beat flat `vec0` at this catalog
   (about 6k videos and 57k chunks) without dropping a filtered neighbor.
   P3 still lists that measurement as open. Flat metadata KNN already filters
   area, source, shelf life, and blocked in SQL.

## Consequences

Search keeps the crates.io extension and the `VectorIndex` trait. When the
fork publishes a crate (or a committed amalgamation) that builds against
rusqlite 0.32 and has a format story, replace `install` and `SqliteVec` only.
