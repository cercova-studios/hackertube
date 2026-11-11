# ADR 0004: Pluggable sources

## Status

Accepted

## Decision

Each catalog is a function `(IngestContext) -> SourceResult` registered in
`sources.SOURCES`. Shipped sources are `hackertube` (the ODC-By dataset) and
`aietalks` (AI Engineer talks). Ingest is idempotent: a matching content
hash returns `changed: false`.

`aietalks` follows the 10x-swe `catalog.py` approach: read the transcript
index, map each row to a YouTube id, fetch `url + ".md"`, and parse
`Source:` / `Published:` when those lines exist. The live transcript index
does not include video ids, so the talks index is the join. Official
ai.engineer markdown omits `Source:`; a YouTube link in the body is accepted
when it matches the join. Fetches are limited to `ai.engineer` and
`aietalks.com`.

## Why this is better for agents

A new source is one module, one dict entry, and one offline fixture. The
agent does not fork the CLI. Re-running ingest cannot duplicate rows or
clobber `state.json`.

## Consequences

Default `aietalks` ingest stops at 20 talks and says how to ask for the rest
(`--limit 0`). The full index is over a thousand markdown files; a silent
full download is a bad first command.
