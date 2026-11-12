# ADR 0006: Declarative focus areas and a separate state file

## Status

Accepted. Ranking by `(-affinity, -score, vid)` is replaced by [0010](0010-recommendation-score.md). Area rules now live in `areas.rs`.

## Decision

Focus areas are phrase, category, and topic-slug rules.
Affinity is an integer. The original rank was `(-affinity, -score, vid)`.

`state.json` stores seen, like, dislike, and snooze. Recommend hides seen,
disliked, and unexpired snoozes. A like is recorded and does not hide the
video. News shelf life is hidden unless `--include-news`.

AI Engineer talks get a stored score from their topic slugs (capped at 9) so
they sit next to mid-ranked HackerTube videos. The Hacker News score is used
unchanged for dataset rows.

## Why this is better for agents

The rules are in one file the agent can read. Ties do not depend on dict
order beyond `vid`. The state file can be edited, diffed, or deleted without
re-ingesting. Hiding likes would throw away a signal the agent just wrote
down; hiding seen videos matches "do not recommend this again".

## Consequences

"Cursor" and "tool use" are not agentic phrases. They match mouse cursors
and stone axes. Agentic coverage in the 2026-10-31 HackerTube export is
thin; AI Engineer talks are the main source for that area.
