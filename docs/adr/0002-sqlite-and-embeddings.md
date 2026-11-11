# ADR 0002: SQLite plus a numpy matrix

## Status

Accepted

## Decision

Video metadata, mentions, co-recommendations, and transcript segments live in
`catalog.sqlite` under `HACKERTUBE_HOME`. Embeddings stay in `embeddings.npy`
(384-d float16, L2-normalized). The row map is the `embed_rows` table.
Agent feedback lives in `state.json`, not in the catalog.

## Why this is better for agents

The files are inspectable. `sqlite3` is the stdlib. Wiping the catalog does
not wipe seen/like/snooze marks, and the reverse is also true. Cosine
similarity is a matrix multiply, which is deterministic and does not need a
vector database running beside the CLI.

## Consequences

Semantic queries need the same BAAI/bge-small-en-v1.5 vectors the dataset
shipped, with no query prefix. fastembed is an optional extra. Without it,
search says `semantic: off` and still answers from FTS.
