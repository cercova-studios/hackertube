# sqlite-vec fork and HNSW benchmark, 2026-11-15

This is the pre-optimization baseline. The subsequent blocked-state change and
rerun are recorded in [the synchronization report](../2026-11-15-blocked-sync/REPORT.md).

## Decision

Keep the pinned fork's flat cosine indexes. Do not add HNSW to production on
this evidence. The fork's exact KNN p95 stays below 3 ms on the tested video
catalog. HNSW makes broad searches faster, but it requires different metadata
access, consumes more worker memory, and trades some neighbor recovery for
speed. Blocked-state synchronization is a much larger cost in the command paths
that call it. `similar` does not perform that synchronization, so this finding
does not explain its latency.

This is a result for a freshly reconstructed video catalog, not the user's
installed catalog or a transcript workload. No populated installed catalog was
found. No production schema, ranking, registration or CLI behavior was changed
by this experiment.

## Corpus and environment

- Official [HackerTube dataset](https://hackertube.org/download/hackertube-data.zip),
  export dated 2026-10-31, downloaded 2026-11-15. ZIP SHA-256:
  `c61bd220d1f3899152f63e6b159eece612f4da49727cc9e970bebba70279892e`.
- 6,454 real video vectors, float32[384] cosine after the application's normal
  float16 decoding. Ingestion used `HACKERTUBE_EMBED=off`, existing dataset
  vectors, and lexical area assignment. No model or captions were fetched.
  There are no transcript chunks or AI Engineer rows in this catalog.
- Apple M5 Pro, 18 logical CPUs, 48 GiB RAM; macOS 27.2 arm64; rustc/Cargo
  1.99.1, Apple clang 21.0.0. These are not low-memory-device measurements.
- Separate release executables, identical rusqlite 0.32.1 / bundled SQLite
  3.46.0. Registry sqlite-vec 0.1.9 versus fork
  `98c0e487fb96acef784ad64dceb78fcb11a55edc` (0.1.10-alpha.4).
  The Cargo binding's normal portable C configuration was used, without adding
  native SIMD flags. The worker fixes SQLite's page cache to 2 MiB and uses WAL.

HackerTube's source data is ODC-By 1.0; credit HackerTube and
[open-index/hacker-news](https://huggingface.co/datasets/open-index/hacker-news).
The dataset, catalogs, vectors and transcripts are not committed to this repo.

## Method and acceptance

The [standalone harness](../../../benchmarks/vector-store/README.md) backs up its
explicit input through SQLite's read-only backup API before changing anything.
Three serial trials use 64 deterministic queries, one warm-up and three measured
rounds: 576 measured observations per case and implementation. Half the queries
use real stored vectors; half blend two vectors. This measures neighbor recovery,
not human-rated relevance or natural-language query inference.

The same simulated blocked set has 645 videos, selected by rowid modulo 10.
Eligibility includes source, news, blocking and area predicates. Before excluding
each query's seed, eligible counts are 6,454 unrestricted, 5,439 for broad catalog
search, 2 agentic, 116 engineering, 99 research, and 0 for the absent AI Engineer
source. Area totals before these filters are 8, 157 and 119 respectively.

Flat SQL uses the existing vec0 metadata constraints and a bound excluded video
ID. HNSW cannot retain those metadata columns. The experimental index instead
uses compact ordinary metadata and `rowid IN` preselection. Unfiltered HNSW
fetches one extra candidate and removes the seed before taking k. No metadata
eligibility filter is applied only after a limited broad search. Both implementations include SQL
preparation and result fetching in query timing; vector preparation is outside.

The experiment's predeclared consideration rule was exact p95 above 20 ms,
at least 2x speedup, at least 0.99 mean and 0.95 minimum tie-aware neighbor recall,
and no eligible-count deficits. This is a decision rule, not a product SLO.
Exact queries did not meet the latency trigger. Higher recall HNSW remains an
option to re-evaluate if a real target workload establishes a need.

## Exact KNN results

Warm query milliseconds, pooled p50 / p95:

| Case | Upstream 0.1.9 | Pinned fork |
|---|---:|---:|
| Similarity, k=6 | 2.303 / 2.636 | 2.471 / 2.884 |
| Catalog, k=80 | 2.571 / 3.166 | 2.413 / 2.827 |
| Agentic area, k=80 | 1.250 / 1.621 | 0.109 / 0.140 |
| Engineering area, k=80 | 1.365 / 1.730 | 1.233 / 1.472 |
| Research area, k=80 | 1.349 / 1.703 | 1.184 / 1.361 |
| Absent source, k=40 | 1.215 / 1.540 | 0.082 / 0.094 |

The fork helps sparse and empty filters most. It is not uniformly faster:
similarity p95 is approximately 9% higher in this run. Both return the same
neighbor identities and counts for every exact-query observation.

An independent float64 oracle checks three queries against all corpus vectors
and validates 744 returned distances per implementation. Maximum absolute error
is 1.022e-6 upstream and 1.488e-8 in the fork. The source's double accumulation
explains the small numerical difference; it is not a neighbor mismatch. Exact
identity checks remain strict, with a separately qualified distance tolerance
of 2e-6. The oracle's eligibility canary rejects an invalid result.

## HNSW results

`m=16`, `ef_construction=200`, 32 MiB cache setting. Warm p95 milliseconds and
mean / minimum recall across the corresponding query observations:

| Search width | Similar k=6 p95 | Similar recall | Catalog k=80 p95 | Catalog recall |
|---|---:|---:|---:|---:|
| ef=64 | 0.257 | 99.74% / 83.33% | 0.883 | 98.09% / 85.00% |
| ef=128 | 0.421 | 99.74% / 83.33% | 1.005 | 99.26% / 93.75% |
| ef=256 | 0.691 | 100% / 100% | 1.277 | 99.88% / 97.50% |

At ef=256, broad catalog search is about 2.2x faster than fork flat search;
similarity is about 4.2x faster. The absolute p95 savings are about 1.6 and
2.2 ms respectively. Lower widths fail the minimum-recall decision rule. No
configuration returns duplicate/ineligible IDs or fewer results than exact
search when eligible rows exist; approximate selections can still miss nearer
neighbors while filling k. Recall is relative to exact vectors, not relevance.

All tested focus-area HNSW queries have 100% recall because the fork explicitly
ranks rowid sets of at most 4,096 exactly. Their ef=256 p95 is 0.237 ms agentic,
0.274 ms engineering and 0.271 ms research. Agentic flat search is faster than
this alternative; the larger engineering and research subsets benefit from
direct exact reads through the graph store. These are not ANN recall wins.

The pinned constructor rejects HNSW metadata columns; the runtime probe records
that error. The implementation and exact-filter threshold can be inspected in
the pinned [constructor](https://github.com/cercova-studios/sqlite-vec/blob/98c0e487fb96acef784ad64dceb78fcb11a55edc/sqlite-vec.c#L6338)
and [HNSW query path](https://github.com/cercova-studios/sqlite-vec/blob/98c0e487fb96acef784ad64dceb78fcb11a55edc/sqlite-vec-hnsw.c#L1385).
Adoption would require metadata/query changes and index lifecycle/recovery work.

## Writes, state synchronization and resources

Median of three trials:

| Operation | Upstream | Fork |
|---|---:|---:|
| 1,000 vector replacements, autocommit | 3.009 s | 3.013 s |
| 1,000 vector replacements, transaction | 2.845 s | 2.867 s |
| Sync 645 blocked marks | 2.417 s | 1.878 s |
| Clear those marks to an empty requested set | 0.525 s | 0.071 s |
| Repeat synchronization with the empty set | 0.618 s | 0.161 s |
| Full offline refresh ingest, 6,454 videos | 23.993 s | 24.221 s |
| Unchanged full ingest, fingerprint no-op | 32.690 ms | 32.155 ms |

The 1,000 replacements reproduce the video-vector delete/insert shape with metadata
supplied by indexed fixture lookups. They are not full ingest throughput:
downloads, parsing, embeddings, ordinary catalog writes and FTS are excluded.
There is no meaningful replacement-throughput gain in this experiment.

Full ingest uses matched release CLI binaries on separate populated catalog
backups, alternating order across three trials. `--refresh --source hackertube`
with the same extracted dataset includes parsing, all source-specific catalog,
vector, mention and related-row writes, and FTS rebuilding. Downloads, extraction,
AI Engineer ingestion and model inference are excluded. Refresh throughput is
about 269 videos/s upstream versus 266 videos/s in the fork; the fork does not
improve this workload. Median refresh peak RSS is approximately 75 MiB in both.
Unchanged input returns `changed: false` in roughly 32 ms. Every run validates
6,454 videos/embeddings and that no user-state file or model cache was created.
The upstream baseline is the repository's pre-integration revision; only
`db.rs`, `vecstore.rs` and the dependency/lockfile differ in application inputs.
Source and binary hashes are in RESULTS.json.

State synchronization executes the existing ordinary/vector reset statements
and per-mark updates. It changes temporary copies only. Empty transitions and
repeated empty state are different measurements; their timing varies with
state/layout and host conditions. Both materially exceed the exact KNN cost.
CPU time is close to wall time in the marked-sync samples, with no major page
faults. No sampled stack profile was taken, so attribution stops at this SQL
phase. Avoiding repeated unchanged blocked-state rewrites is the next candidate
to investigate, not an implemented optimization.

HNSW construction takes 2.94–3.12 s. Its sidecar adds 14 MiB relative to the
captured template, while the flat index remains for comparison. That is not the
size of a migrated production database. Incremental HNSW churn is unmeasured.

Query-worker peak RSS is about 23–27 MiB for the exact variants versus about
34 MiB for 32 MiB-cache HNSW, including retained benchmark requests/results.
These are worker high-water marks, not the full application's memory cost.
An 8 MiB HNSW setting slows broad-query p95 to 2.006 ms and has peak RSS between
31 and 40 MiB: the cache setting is not a strict process-memory ceiling.

Fresh-connection/process samples do not purge the OS cache. There are only three
per case, insufficient for a reliable startup-tail comparison. Most first
similarity queries plus opening are 1.7–8.5 ms; one HNSW ef=64 sample is 78.6 ms.
Do not interpret this outlier as a proven HNSW startup regression. ef=256 fresh
startup was not measured, and no full search CLI/model latency comparison was performed.

## Evidence, reproduction and limits

[RESULTS.json](RESULTS.json) retains aggregate measurements, per-case recall,
resource samples, input/source hashes and the independent oracle result.
The complete protocol, queries and raw observations are retained at
`/private/tmp/hackertube-benchmark-20261115/run-4/`.
The source catalog and original ZIP are in the parent benchmark directory.
These are disposable local artifacts, not an installed application home.

From the repository root, repeat using a fresh destination:

```sh
python3 benchmarks/vector-store/run.py \
  --source /private/tmp/hackertube-benchmark-20261115/catalog/catalog.sqlite \
  --output /private/tmp/hackertube-vector-repeat
python3 benchmarks/vector-store/extras.py /private/tmp/hackertube-vector-repeat
python3 benchmarks/vector-store/verify.py /private/tmp/hackertube-vector-repeat
```

Full-ingest replay additionally needs the two matched binaries; use the
`ingest.py` command in the benchmark README. Recorded binaries, inputs and
`full-ingest/` raw per-child timing/RSS records remain under the local benchmark
directory. The standalone harness does not build the full application for this
supplement; its exact commands are retained with the measurements.

The corrected final run binds known video IDs directly and indexes fixture
identity lookups. Pilot measurements with a wide metadata scan or unindexed
fixture lookups are excluded. This prevents harness work from being credited as
an HNSW speedup. Independent exact-neighbor assertions and the oracle remain on.

Limits: one host, one real video corpus, no installed user marks, no transcript
vectors, no natural-language relevance judgments, no cold-disk control, no
AI Engineer ingest or search CLI/model comparison, no graph-update failure/lifecycle qualification
and no low-memory-target run. Three process trials expose variation but do not
establish independent confidence intervals or production performance guarantees.
