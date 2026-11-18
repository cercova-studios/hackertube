# Compare vector stores on a catalog snapshot

This local experiment compares registry sqlite-vec 0.1.9 with the pinned fork
using the same bundled SQLite and release build. It measures video KNN,
vector replacement and blocked-state synchronization. `ingest.py` separately
measures the full offline CLI ingest with explicit matched binaries. Neither
experiment measures embedding inference or transcript retrieval.

Requires Cargo, a C compiler and Python 3 with sqlite3. No model is loaded.
Cache the standalone worker dependencies before disconnecting:

```sh
cargo fetch --locked --manifest-path benchmarks/vector-store/Cargo.toml
python3 benchmarks/vector-store/run.py \
  --source /absolute/path/to/catalog.sqlite \
  --output /absolute/path/to/new-benchmark-directory
python3 benchmarks/vector-store/extras.py /absolute/path/to/new-benchmark-directory
python3 benchmarks/vector-store/verify.py /absolute/path/to/new-benchmark-directory
```

The output directory must not exist. The driver opens the source read-only and
uses SQLite backup to capture committed WAL contents. Only disposable copies are
modified. Builds, databases, requests, results and the protocol stay in the
output directory. A failure retains evidence; use a fresh output directory to
retry. `extras.py` also requires fresh supplemental filenames. Do not invoke the
internal Rust worker directly against a user catalog: its operations write data.

## Workload and checks

- Three trials, 64 deterministic queries, one warm-up and three measured rounds.
  Half the queries are existing vectors; half blend two stored vectors. These
  test neighbor recovery, not natural-language relevance.
- Similarity requests six candidates and excludes the seed video. Catalog
  requests use 80 candidates, with blocked, news and focus-area predicates.
  An absent AI Engineer source exercises explicit emptiness in a HackerTube-only
  corpus. Corpus counts and exact eligibility are recorded in `protocol.json`.
- A simulated 10% blocked set is applied consistently to both stores. The driver
  never reads or modifies the user's `state.json`. `extras.py` measures zero
  marks separately and adds `ef_search=256` trials.
- HNSW uses `m=16`, `ef_construction=200`, search widths 64/128/256 and a
  32 MiB cache setting; an 8 MiB setting is also sampled. Unfiltered queries
  fetch one extra candidate before removing self. Filtered queries preselect
  rowids from compact ordinary metadata. These are experimental access paths,
  not changes to HackerTube's production schema.
- Each exact result must match upstream IDs and counts. Distance tolerance is
  2e-6, qualified separately with the float64 oracle. ANN checks uniqueness,
  eligibility, result-count deficits and recall against exact results. Missing
  eligible results count as errors; empty ground truth has recall 1.

`verify.py` independently computes float64 distances over all corpus vectors for
three selected queries and checks both exact implementations against them. Its
eligibility canary must reject an invalid result. The full experiment fails on
an exact mismatch rather than silently continuing.

## Interpreting measurements

Query durations include SQL preparation and result fetching, with vector
preparation outside the timer. Fresh-process samples include connection opening
and the first query; the OS page cache is uncontrolled. They are not cold-disk
measurements. Peak RSS includes the worker and retained result records. The
cache setting alone is not a process-memory limit.

Replacement timings cover 1,000 video-vector delete/insert cycles, with known
IDs and vectors prepared before the timer. Indexed fixture lookups supply the
remaining metadata. The autocommit case follows the current vector write shape;
the transaction case is a comparison, not a production ingest change. Mark
synchronization in the worker's `legacy_sync` operation deliberately retains
the pre-optimization SQL (resets and per-video updates) for engine comparisons.
It does not measure the application's optimized synchronization. Use
`blocked.py` below for that comparison. HNSW index construction is measured
separately; graph update churn is not covered.

Raw timings in `*.json`, `summary.json`, `extras.json` and `oracle.json` are the
evidence. Trials run serially; avoid competing builds or workloads. Pooled
percentiles describe this workload and host, not independent confidence
intervals. The 20 ms p95 / 2x speed / 0.99 mean and 0.95 minimum recall gate is an
experiment decision rule, not an accepted product service-level objective.

The November 2026 findings are recorded in
[the development report](../../docs/development/2026-11-15-vector-benchmark/REPORT.md).

## Production blocked-state synchronization

`examples/blocked_sync.rs` calls the actual `db::sync_blocked` implementation.
Capture its release executable and the CLI before and after the change, using
the same pinned extension and dependency lockfile. Build with
`cargo build --release --offline --locked --example blocked_sync --bin hackertube`.
Keep the before source snapshot for reproduction; do not replace a captured
before executable with a rebuilt candidate.

```sh
python3 benchmarks/vector-store/blocked.py \
  --source /absolute/path/to/catalog.sqlite \
  --output /absolute/path/to/new-comparison-directory \
  --before /absolute/path/to/before-blocked-sync \
  --after /absolute/path/to/after-blocked-sync \
  --cli-before /absolute/path/to/before-hackertube \
  --cli-after /absolute/path/to/after-hackertube
```

This fixed-corpus experiment uses the 6,454-video snapshot and 645 simulated
marks. It compares empty, first-mark, repeat, partial-unblock and clear cases
on fresh backups across three trials. It asserts zero writes for unchanged
candidate sets and identical recommendation JSON from both CLIs. It writes
fixture `state.json` files only in the new comparison directory. No transcript
vectors are present in this performance corpus; offline unit tests cover them.

## Full offline ingest

Supply release binaries built from matched upstream/fork application sources and
an already extracted dataset. The experiment creates `full-ingest/` under the
completed run directory; that subdirectory must not exist. It performs three
forced refreshes and unchanged no-op checks per implementation, each on a
separate catalog backup. Models and captions are disabled. It asserts successful
row/embedding counts and that no state file or model cache was created.

```sh
python3 benchmarks/vector-store/ingest.py \
  --run /absolute/path/to/completed-benchmark-directory \
  --source /absolute/path/to/catalog.sqlite \
  --data /absolute/path/to/extracted-hackertube-data \
  --fork-bin /absolute/path/to/fork-hackertube \
  --upstream-bin /absolute/path/to/upstream-hackertube
```

The unchanged-catalog route measures fingerprint-based no-ops. The refresh route
includes parsing, vector/ordinary table writes, mentions, related rows and FTS
rebuild. It excludes download, archive extraction and model inference. Per-child
CPU and peak RSS use Unix `wait4`; this supplemental driver requires a Unix host.
