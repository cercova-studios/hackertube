---
name: add-source
description: Add a HackerTube video source plugin end to end, including the offline fixture test. Use when ingesting a new catalog of videos alongside hackertube and aietalks.
---

# Add a source

Sources are functions dispatched from `ingest_source` in `src/ingest.rs`. `ingest --source <id>` calls the one you add. The existing pair is `hackertube` and `aietalks`. Copy `ingest_aietalks` for a network catalog and `ingest_hackertube` for a local dump.

## 1. Function

Add `ingest_<id>(ctx: &IngestContext) -> Result<SourceResult, AppError>` in `src/ingest.rs` and a match arm in `ingest_source`.

`IngestContext` gives you `catalog`, `home`, `path`, `limit`, `refresh`, and `embedder`.

Return `SourceResult { source, videos, embedded, changed, detail }`.

Rules:

- Upsert with `db::upsert_video`. `source` must be your id. `vid` is an 11-character YouTube id. `hn_recommenders` is `0` for non-HN catalogs so you are not filtered as "never recommended" (that filter applies only to `source == "hackertube"`).
- Put focus signals in `topic_slugs` and/or `categories`. `areas::classify` reads those, plus cosine similarity to the area seed texts when the embedder is ready. A hard block (hide-and-seek, stone axe, comedy, music) stays at affinity 0 even if the cosine is high.
- Store a grounded `why` string copied from the upstream summary. `areas::reason` will not invent one.
- Assign a deterministic `score` (float). Higher is better. Do not use randomness.
- Cache bytes under `ctx.home.join("cache").join("<id>")`. On `refresh == false`, reuse the cache. A second ingest with the same bytes returns `changed: false`. Store a sha256 via `db::meta_set`.
- If you have transcript text, write `transcripts/<vid>.md` and index chunks with `db::replace_chunks` plus `VectorIndex::upsert_chunk`. Embed with `ctx.embedder.embed` (no query prefix). If the embedder is off, still save the videos and report `embedded: 0`.
- Insert video vectors through `VectorIndex::upsert_video`. Do not write `vec0` SQL from the new source.
- Refuse URLs whose host you did not list. Return `AppError`, do not prompt.
- Do not read or write `state.json`.

## 2. Register

Add the match arm in `ingest_source` and the clap `value_parser` list on `Ingest.source` in `src/cli.rs`. `all` should call it. If `--path` means the HackerTube zip, do not pass that path through to your source unless `--source` is exactly your id. See `ingest_cmd` in `src/commands.rs`.

## 3. Area coverage

Run a video from the fixture through `areas::classify`. If every row scores `0`, recommend will hide the source. Fix the stored topics or the area rules, and add a unit test in `src/areas.rs`.

## 4. Offline test

Add a fixture directory the test writes under `tmp_path` (json and markdown, no zip of the real dataset). Ingest with:

```rust
Command::cargo_bin("hackertube").unwrap()
    .env("HACKERTUBE_EMBED", "off")
    .env("HACKERTUBE_HOME", &home)
    .args(["ingest", "--source", "<id>", "--path", fixture.to_str().unwrap()])
    .assert()
    .success();
```

Assert:

- exit 0 and the row contains `true` the first time
- the second call prints `false` in the `changed` column
- `recommend` or `show` sees a vid from the fixture
- the test does not download (`HACKERTUBE_EMBED=off`, local `--path` only)

Also assert a bad host or a path escape returns `error: unsafe_zip` or `disallowed_url` if your loader accepts a path.

## 5. Docs

One paragraph in `AGENTS.md` under "How to run" if the agent must pass a new flag. An ADR if you change the schema or the default ingest set. Do not commit fetched payloads.
