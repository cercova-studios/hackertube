# ADR 0003: TOON by default

## Status

Accepted

## Decision

Stdout is TOON unless `--json` is passed. List commands default to 3–4
columns. Long text truncates with `…[+Nc]`. Counts are printed as
`shown of matched`. Empty results name the miss. Errors use the same
stdout document with `ok: false` and `exit:`. Unknown flags exit 2.
Mutations report `changed`.

## Why this is better for agents

The [AXI](https://axi.md) measurements are the reason: a short, declared
table is cheaper and less ambiguous than a JSON blob or a human paragraph.
`help[N]` after every document tells the agent the next command without a
second `--help` call. Idempotent `changed: false` means a retry is safe.

## Consequences

`output.py` is the only serializer. Commands build a flat dict and a
`Table`. Insight tables do not share columns with video tables, so they
ignore `--fields` rather than erroring.
