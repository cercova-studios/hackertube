# ADR 0007: Rust binary, one implementation

## Status

Accepted

## Decision

HackerTube is a single Cargo binary, `hackertube`. The Python package is gone.
The CLI verbs, TOON/JSON contract, exit codes, and `HACKERTUBE_HOME` layout
stay. A catalog written by the Python build (`schema_version` 1) is moved to
`catalog.sqlite.v1` and recreated; `state.json` is left in place.

## Why this is better for agents

One binary, no virtualenv, and the same stdout contract. Agents do not have to
guess which implementation is installed. Semantic search and transcript
chunking sit in the same process as the SQL filters.

## Consequences

Install is `cargo install --path .` or `cargo build --release`. Tests are
`cargo fmt`, `cargo clippy -D warnings`, and `cargo test`. `HACKERTUBE_EMBED=off`
skips the model download so tests stay offline.
