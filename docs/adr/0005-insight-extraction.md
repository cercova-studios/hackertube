# ADR 0005: Python insight extraction, not distiller

## Status

Accepted

## Decision

`insights` and `transcript` are implemented in Python. They do not shell out
to the 10x-swe `distiller` binary.

The distiller's YouTube path (`plugins/knowledge/tools/distiller/src/extractors/youtube.rs`,
dispatched from `registry.rs`) checks the host and returns the page title
plus a chunk of description HTML. It does not return timestamped claims,
chapters, or repo/paper mentions. Calling it would add a Rust binary agents
often do not have, and a missing binary would change results.

This CLI parses AI Engineer / AIE Talks markdown and YouTube caption cues.
Chapters, claims, tool/paper/repo mentions, and quotes each keep `at` and a
YouTube `url`. Selection is time order and string match, not a model.
`--llm` runs only when `HACKERTUBE_LLM_API_KEY` is set, and a model claim is
kept only when its text is a verbatim substring at a timestamp that exists
in the transcript.

## Why this is better for agents

The same fixture produces the same JSON every time, with no binary and no
API key. Provenance is on the item, so the agent can open the second in the
video instead of trusting a paraphrase.

## Consequences

Caption download needs the `captions` extra and a network. Failure is
`transcript_unavailable` on stdout, exit 1, not a stack trace.
