# ADRs

| ADR | Decision |
|---|---|
| [0001](0001-agent-first-cli.md) | CLI only. No web UI |
| [0002](0002-sqlite-and-embeddings.md) | Superseded. State file stays; vectors moved to sqlite-vec |
| [0003](0003-toon-output-contract.md) | TOON stdout, AXI field and error rules |
| [0004](0004-pluggable-sources.md) | Source plugins, including AI Engineer talks |
| [0005](0005-insight-extraction.md) | In-process insight extraction, not distiller |
| [0006](0006-focus-areas-and-state.md) | Focus areas and state. Ranking superseded by 0010 |
| [0007](0007-rust-port.md) | Single Rust binary; Python package removed |
| [0008](0008-sqlite-vec.md) | sqlite-vec KNN behind `VectorIndex`; lazy bge-small embeddings |
| [0009](0009-sqlite-vec-fork.md) | Stay on upstream sqlite-vec until the fork builds as a crate |
| [0010](0010-recommendation-score.md) | Continuous recommend score; ties are not broken by video id |
