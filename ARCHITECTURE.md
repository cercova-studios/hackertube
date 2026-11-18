# HackerTube architecture

## 1. Purpose and scope

HackerTube is a local CLI for coding agents to discover AI-engineering videos
and retrieve grounded transcript evidence. It has no server or web viewer.
The binary and library are built from the same Cargo package.

## 2. Entrypoints and dependency direction

`src/main.rs` calls `cli::run`. `src/cli.rs` parses arguments and dispatches to
`src/commands.rs`. Command handlers coordinate the catalog, marks, embedding,
ingest and extraction modules, then render through `src/output.rs`.

`src/lib.rs` exports these modules. Storage and extraction do not dispatch CLI
commands. `src/error.rs` provides `AppError` and `UsageError` across boundaries.
There is no runtime plugin registry.

## 3. Catalog and persistence

`src/db.rs::Catalog::open` owns connection setup and schema initialization.
The catalog uses WAL and schema version `2`. Ordinary tables contain videos,
mentions, related video IDs, transcript chunks, video embedding copies and meta
values. FTS5 tables index videos and chunks. `src/vecstore.rs` owns vector table
creation, writes and KNN queries; `db::replace_chunks` also deletes obsolete
chunk vectors when replacing transcript text.

`Catalog::open` checks an existing schema marker before initialization. A
different nonempty marker takes the legacy rename-to-`catalog.sqlite.v1` path.
That path ignores rename errors; it is not a general migration or backup system.

## 4. Files and configuration

`src/paths.rs` resolves `HACKERTUBE_HOME`, defaulting to `~/.hackertube`.
The catalog, `state.json`, transcript markdown, model cache and downloaded
source caches live there. Explicit `setup` also writes integration files under
the user's home; tests redirect both `HOME` and `HACKERTUBE_HOME`.
`HACKERTUBE_TODAY` freezes dates for replay, including snooze expiration.

## 5. Domain types

| Type | Owner and responsibility |
|---|---|
| `Video` | `db.rs`: catalog metadata, source, transcript path and area affinities |
| `VideoVec`, `ChunkVec` | `vecstore.rs`: vector payload, stable row identity and filter metadata |
| `KnnFilter`, `VideoHit`, `ChunkHit` | `vecstore.rs`: eligible neighbor requests and distance results |
| `EmbedStatus`, `Embedder` | `embed.rs`: available model or explicit fallback notice |
| `Signals`, `Affinity` | `areas.rs`: lexical signals and area eligibility |
| `Mark` | `state.rs`: seen, liked, disliked or snoozed status |
| `IngestContext`, `SourceResult` | `ingest.rs`: source inputs and idempotency result |
| `Segment` | `insights.rs`: timestamped transcript evidence |
| `Flags`, `Doc`, `Val` | `commands.rs`, `output.rs`: rendering options and output documents |

## 6. Vector and embedding boundary

`VectorIndex` defines schema creation, video/chunk upserts, filtered KNN and
blocked-state synchronization. `SqliteVec` implements it on a borrowed SQLite
connection. Both vector tables use flat float32 vectors of dimension 384 and
cosine distance. Video rows carry shelf life; chunk rows carry start seconds.
They both carry video ID, source, area affinities and blocked state.

The sqlite-vec dependency is pinned to a reviewed commit of the
`cercova-studios/sqlite-vec` fork. Its C core is linked statically;
rusqlite's `bundled` feature supplies the single SQLite runtime. Each catalog
connection registers the extension before accessing virtual tables. No dynamic
extension path or Lance runtime is required. See ADR 0009 for compatibility and
rollback rules.

`embed.rs` loads bge-small-en-v1.5 lazily, tries the quantized and full models,
and normalizes new embeddings. Model identity participates in ingest
fingerprints. Model failure or `HACKERTUBE_EMBED=off` selects lexical fallback.
Stored HackerTube float16 vectors are decoded to float32 during ingestion.

## 7. Ingest flow

`ingest_source` selects the closed set of source handlers. HackerTube ingestion
reads local JSONL and vector files or downloads the dataset archive. AI Engineer
ingestion joins transcript and talk indexes by slug, validates video provenance,
stores markdown, chunks it and writes text/FTS/vector rows. Network fetching and
archive validation live in `ingest.rs`; offline tests supply local snapshots.

Fingerprints include source input and model identity. Successful unchanged
ingestion is a no-op. Ingestion does not write `state.json`. Current ingestion
uses multiple database statements without an enclosing all-or-nothing source
transaction; a failed run can leave partial catalog work. Transcript files and
SQLite are also separate persistence boundaries.

## 8. Search, recommendation and extraction

`commands.rs::search` combines normalized FTS scores with cosine similarity
(0.7 semantic, 0.3 lexical when semantic search runs). Catalog and transcript
search have separate candidate and merge paths. Vector constraints execute
inside KNN. Lexical filtering is a separate caller responsibility: catalog
search applies source/area after its FTS limit, while the transcript lexical
branch currently does not apply those filters. Do not infer whole-search
eligibility guarantees from the vector filters alone.

`similar` uses stored video embeddings when present, otherwise co-recommendation
edges. `recommend` ranks catalog rows with area/state eligibility rather than
running a model at ranking time. `insights.rs` deterministically extracts
chapters and evidence. The explicit `insights --llm` path requires its API key.

## 9. Extension points

1. Commands: add a clap variant and handler; follow `.agents/skills/add-command`.
2. Sources: add a handler and `ingest_source` match arm; follow
   `.agents/skills/add-source`. Keep network fixtures offline.
3. Vector implementation: preserve `VectorIndex`, cosine semantics, identities,
   metadata constraints and transaction behavior. Keep KNN SQL in `vecstore.rs`.
4. Areas: change `areas.rs` rules, seeds and the three explicit area columns
   together; review schema, ranking and fixtures before changing the contract.

## 10. Invariants and qualifications

- Ranking, extraction and rendering are deterministic functions of inputs; no
  hidden LLM call belongs in them.
- `state.json` owns user marks. Eligible reads copy blocking state into ordinary
  and vector tables. Seen/disliked/active snoozes block recommendation; likes do
  not. The vector copy is derived state, not a second mark authority.
  Synchronization binds the desired IDs as JSON and updates only flags that
  differ, in one statement per table. It does not cache a mark fingerprint:
  newly ingested rows and snooze expiration are reconciled on the next read.
  Statement atomicity and caller-managed transaction rollback remain unchanged;
  the three table statements are not a new all-or-nothing transaction.
- Video IDs and chunk rowids retain their identity across relational and vector
  results. Chunk hits retain timestamps and evidence URLs.
- Existing command fields, exit codes, cosine scoring, model identity and
  schema version remain stable when changing the vector dependency alone.
- SQL rollback and savepoints govern vector changes within a transaction; this
  does not make the multi-statement ingest workflow atomic.

## 11. Output and failure behavior

`output.rs` emits TOON by default and JSON for `--json`. Default list rows have
three or four fields; `help` is last. Operational errors produce stdout documents
and exit 1; usage errors exit 2; empty results and idempotent no-ops exit 0.
There are no prompts or pagers. Clap handles help separately. See ADR 0003 and
AGENTS.md for exact rendering and truncation rules.

## 12. Build and verification

Use stable Rust, Cargo and a C compiler. The package declares Rust 1.85 as its
minimum version. `cargo build --release` builds the CLI. Required checks are
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.
CI runs these checks on Linux. Tests disable embedding downloads and captions
and use temporary homes; fixed vectors exercise the native KNN path offline.

`benchmarks/vector-store/` is a separate, optional video-vector experiment. It
compares isolated upstream/fork release workers on SQLite backups without
changing the application's dependency or schema. Its protocol and development
results are linked from the benchmark README; it is not a required CI benchmark.
Its `legacy_sync` operation retains the earlier SQL for engine comparisons.
`examples/blocked_sync.rs` calls production synchronization for paired before/after
application benchmarks.

## 13. Operation and recovery

The executable opens the local catalog directly. Back up a closed catalog, or
use a SQLite-consistent backup that includes committed WAL contents; copying
only a live main database file is insufficient. Keep marks and transcripts with
the catalog backup. Restoring the pre-upgrade catalog plus the old executable is
the supported dependency rollback. Fork-written numeric metadata zone maps must
be cleared under the fork before allowing an older writer to modify that same
database; see ADR 0009. A dependency upgrade alone needs no re-embedding.

## 14. How to keep this document honest

Update this document in the same change when modifying module responsibilities,
domain types, source/command dispatch, persistence paths, schema, vector format,
registration, model identity, filtering/ranking, output or recovery contracts.
Update the relevant ADR and offline tests when a decision changes. Keep plans,
experimental measurements and validation history outside current-capability docs.
