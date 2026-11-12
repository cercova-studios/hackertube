# ADR 0010: Continuous recommendation score

## Status

Accepted

## Decision

`recommend` prints a score computed at read time, not the stored catalog
score and not a sort by video id.

The blend is similarity to that area's seed text, keyword affinity,
source quality (stored score and recommender count), shelf life plus publish
year, and, when the user has embedded likes or dislikes, similarity to that
centroid minus similarity to the dislike centroid. Missing embeddings drop
the seed and profile terms and give their weight to the seed slot so keyword
and quality still order a catalog built with `HACKERTUBE_EMBED=off`.

Remaining ties break on affinity, seed similarity, source quality, freshness,
then title. The video id is not a sort key.

Agentic engineering keeps a video only when a strong keyword or slug is
present and the seed cosine beats AI engineering and AI research by a
margin. A weak keyword is promoted only with a clearer margin. A hard block
(hide-and-seek, comedy, music) stays at zero even when the cosine is high.
The classifier version is stored in `meta.areas_version`. A model change
recomputes areas from the stored vectors without re-downloading transcripts.

## Why this is better for agents

A flat score of 7 sorted by id put every id that starts with `-` first,
including talks that are not about coding agents. The new number differs
across rows, and the order follows the blend. The reason string is still
copied from stored text.

## Consequences

`show` still prints the stored source score. `recommend` prints the blend
with up to three decimal places. The first command after a classifier change
rewrites area columns and the `vec0` metadata. Later commands see a matching
`areas_version` and skip that pass.
