# ADR 0008: sqlite-vec behind a vector-index trait

## Status

Accepted

## Decision

Video and transcript-chunk embeddings live in `vec0` tables (384-d float,
cosine distance) via the `sqlite-vec` crate. `vecstore::VectorIndex` is the
only API search and ingest call. `SqliteVec` is the current implementation.
Replacing it with a drop-in fork (github.com/cercova-studios/sqlite-vec, today
the same C API as upstream) means changing `install` and `SqliteVec`, not the
queries' callers.

KNN queries pass area, source, shelf life, and blocked (seen / disliked /
active snooze) as `vec0` metadata constraints, so the filter happens in SQL
during the search. `state.json` remains the source of truth; each read copies
the blocked set into the `blocked` column before the query.

New text is embedded with `bge-small-en-v1.5` (384-d, no query prefix) and
L2-normalized. The loader tries fastembed's quantized ONNX
(`BGESmallENV15Q`, the build named in the dataset's DATA.md) and, if a probe
embed fails, the full `Xenova/bge-small-en-v1.5` export. ort 2 rejects the
quantized file (`SkipLayerNormalization` is missing an initializer). The full
export matches stored dataset rows at about 0.94 cosine, so those float16
vectors stay in the index. The chosen id is written to `models/selected-model`
and into the catalog fingerprint, so a later model change re-embeds. The
download is lazy under `HACKERTUBE_HOME/models`. If neither model can run,
search says `semantic: off` plus a `notice` and uses FTS only. Similarity
promotes a video into a focus area only when a keyword signal is already
present; it does not override a hard block.

## Why this is better for agents

Neighbor search is one SQL statement with the same filters as recommend, not a
Python loop over a numpy matrix. The trait keeps the fork option local. A
missing model does not fail the command.

## Consequences

The first ingest that embeds the AI Engineer catalog downloads the ONNX model
and writes one vector per video plus one per transcript chunk (~800
characters). Re-running ingest with the same bytes and embedder is a no-op
(`changed: false`).
