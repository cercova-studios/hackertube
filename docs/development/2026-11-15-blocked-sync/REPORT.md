# Blocked-state synchronization optimization, 2026-11-15

## Outcome

Replace reset-and-mark loops with one set-based UPDATE per table, guarded so
unchanged flags are not written. On the same 6,454-video snapshot, repeated
synchronization with 645 marks falls from 1,906 ms to 7.87 ms, approximately
242x faster. Repeated recommendation CLI calls fall from 1,910 ms to 19.2 ms,
approximately 99x faster, with identical JSON documents.

This change introduces no schema migration, mark-format change, dependency,
HNSW index, ranking change or cached mark fingerprint. See
[ADR 0010](../../adr/0010-blocked-state-reconciliation.md).

## Mechanism and scope

Previously `db::sync_blocked` and `SqliteVec::set_blocked` reset all flags, then
issued ordinary/vector UPDATEs for each marked video. Repeated calls rewrote
unchanged flags and repeatedly searched vector metadata by video ID.

The new statements bind the desired IDs as a JSON array. SQLite computes set
membership through `json_each`, then writes only rows whose flags differ.
Each statement uses one parameter, and the JSON array grows with the set size.
Every call still re-evaluates current rows; newly inserted chunks and re-ingested
vectors are repaired without an invalidation cache. Existing snooze, dislike,
seen, like and forget rules remain in `state.rs`.

SQLite statement atomicity and caller transaction rollback are retained. As
before, a failure between the three table updates can leave partial derived
state; this change does not add a cross-table transaction. SQL errors propagate,
and the next successful synchronization reconciles the flags again.

## Paired production benchmark

Both before and after builds use the same fork revision
`98c0e487fb96acef784ad64dceb78fcb11a55edc` and identical dependency lockfiles.
The before source was captured from the working tree before editing. This
isolates the algorithm change from the earlier upstream-to-fork change.
`examples/blocked_sync.rs` invokes the actual production function rather than a
second SQL implementation. The standalone engine worker retains explicitly
named `legacy_sync` SQL for historical engine comparisons only.

Hardware and corpus match the [earlier study](../2026-11-15-vector-benchmark/REPORT.md):
Apple M5 Pro, 18 logical CPUs, 48 GiB RAM, macOS 27.2 arm64; release builds,
bundled SQLite 3.46.0, 2 MiB SQLite page cache. The public dataset contains 6,454
video vectors and no transcript chunks. The simulated 645 marks select video
embedding row IDs divisible by ten. No installed user catalog or marks are used.

Three trials alternate implementation order. Every trial starts with a fresh
SQLite backup, then executes the same sequence of desired sets. Times below
are medians in milliseconds, with no competing task-owned builds during timing:

| Transition | Before | After |
|---|---:|---:|
| Empty desired set, already empty | 69.96 | 6.48 |
| First application of 645 marks | 1,894.93 | 56.58 |
| Repeat the same 645 marks | 1,906.40 | 7.87 |
| Remove 322 marks, retaining 323 | 982.01 | 23.21 |
| Clear the remaining marks | 71.00 | 22.65 |
| Repeat the empty set | 63.12 | 6.43 |
| Recommendation CLI after marking | 1,892.38 | 69.22 |
| Repeat recommendation CLI | 1,910.21 | 19.22 |

All candidate empty/repeat observations record zero SQLite changes inside the
synchronization function, including shadow-table changes. This does not claim
that the entire command performs no writes: catalog initialization remains a
separate phase. Both CLI variants return identical recommendation JSON in all
six paired observations; result hashes are retained in RESULTS.json.

First-command timing varies: one candidate first recommendation is 337.75 ms.
There are only three trials per transition, so these results do not establish a
startup-tail or production latency guarantee. Raw samples, not only medians,
are retained.

## Broader benchmark rerun

The original controlled vector workload was rerun with three trials, 64 queries,
one warm-up and three measured rounds. All exact result IDs, counts and distance
checks pass. Fork warm p95 is 2.45 ms for similarity, 2.42 ms for broad catalog,
0.124 ms agentic, 1.46 ms engineering, 1.24 ms research and 0.114 ms absent source.
The KNN code is unchanged; timing variation is not credited to synchronization.

HNSW ef=256 again has 100% similarity recall and 99.88% mean / 97.5% minimum
broad-catalog recall. No duplicate/ineligible IDs or eligible-count deficits
were observed. Its broad-catalog p95 is 1.20 ms. The HNSW integration decision
remains deferred: it requires different metadata access and adds approximation
and memory costs for a small absolute KNN saving.

The independent float64 oracle again checks three queries over the full corpus
and 744 returned distances per exact implementation. Maximum error remains
1.022e-6 upstream and 1.488e-8 in the fork. The oracle's invalid-eligibility
canary is rejected.

The full offline refresh/no-op ingest benchmark was also rerun on disposable
copies across three trials. Median full refresh is 23.10 s upstream and 23.61 s
in the optimized fork; unchanged ingests are 30.90 and 31.63 ms. Ingest does not
call blocked-state synchronization, so no ingest speedup is claimed. Every run
validates 6,454 videos/embeddings and absence of a new state file or model cache.

## Regression evidence and checks

The no-op regression was run before the patch and failed for the intended
reason: total changes increased from 104 to 124 when identical marks were
re-applied. It passes after the patch without changing its zero-write condition.
The offline fixtures also cover duplicate/unknown quoted IDs, new transcript
chunks, stale flags after ingestion, clearing marks, likes, dislike removal,
snooze expiration and caller savepoint rollback across all three tables.

Checks run against the candidate:

- `cargo fmt --check`
- `cargo clippy --offline --locked --all-targets -- -D warnings`
- `cargo test --offline --locked`: 17 tests passed
- Standalone benchmark formatting and strict Clippy in fork and upstream modes
- Independent cosine oracle, input/output assertions, Python syntax and local
  documentation-link checks

These are local checks. Hosted CI, real user-state contention and transcript-scale
performance were not exercised. Unit fixtures do exercise transcript metadata.

## Reproduction and retained evidence

[RESULTS.json](RESULTS.json) stores every production timing observation, output
hashes, before/after source and binary hashes, lockfile equality, vector rerun
summary, full-ingest records and the oracle result. Raw inputs and observations
are retained under `/private/tmp/hackertube-blocked-sync-20261115/`:

- `before-source/`, `blocked-before`, `blocked-after`, `cli-before`, `cli-after`
- `comparison/records.json` and `comparison/summary.json`
- `vector-rerun/`, including supplemental HNSW and full-ingest records

Use the [benchmark instructions](../../../benchmarks/vector-store/README.md) to
build captured before/after examples and run `blocked.py`. The source catalog,
ZIP and extracted public dataset remain in
`/private/tmp/hackertube-benchmark-20261115/`. Original dataset attribution and
SHA-256 are recorded in the earlier study; no dataset is committed here.
