---
name: add-source
description: Add a HackerTube video source plugin end to end, including the offline fixture test. Use when ingesting a new catalog of videos alongside hackertube and aietalks.
---

# Add a source

Sources are functions registered in `src/hackertube/sources/__init__.py`. `ingest --source <id>` calls the one you add. The existing pair is `hackertube` and `aietalks`. Copy the shape of `aietalks.py` for a network catalog and `hackertube_dataset.py` for a local dump.

## 1. Module

Create `src/hackertube/sources/<id>.py`.

```python
def ingest_<id>(ctx: IngestContext) -> SourceResult:
    ...
```

`IngestContext` (`sources/base.py`) gives you `conn`, `home`, `path`, `limit`, and `refresh`.

Return `SourceResult(source, videos, embedded, changed, detail)`.

Rules:

- Upsert with `db.upsert_video`. The column set is the dict keys in `db.upsert_video`. `source` must be your id. `vid` is an 11-character YouTube id. `hn_recommenders` is `0` for non-HN catalogs so you are not filtered as "never recommended" (that filter applies only to `source == "hackertube"`).
- Put focus signals in `topics_json` (list of slugs) and/or `categories_json` (list of HackerTube category ids). `areas.py` reads those. If your topics are new, add slugs to the matching `Area.topic_slugs` in the same change and mention it in the ADR only if the area definition changed meaning.
- Store a grounded `why` string copied from the upstream summary or blurb. `rank.reason_for` will not invent one.
- Assign a deterministic `score` (float). Higher is better. Do not use randomness.
- Cache bytes under `ctx.home / "cache" / "<id>"`. On `refresh=False`, reuse the cache. A second ingest with the same bytes returns `changed=False`. Store a sha256 in `meta` via `db.meta_get` / `db.meta_set`.
- If you have transcript text, write it with `transcript.write_cached` and index segments with `db.replace_segments` using `insights.parse_markdown`.
- Embed with `embed.embed_texts` (no query prefix) and `embed.merge_rows`, then `db.set_embed_row`. If `embed_texts` returns `None`, still save the videos and report `embedded=0`.
- Refuse URLs whose host you did not list. Raise `AppError`, do not prompt.
- Do not read or write `state.json`.

## 2. Register

In `sources/__init__.py`:

```python
SOURCES = {
    "hackertube": ingest_hackertube,
    "aietalks": ingest_aietalks,
    "<id>": ingest_<id>,
}
```

`cli.py` `--source` choices must include the new id (`hackertube`, `aietalks`, `all`, and your id). `all` should call it. If `--path` means the HackerTube zip, do not pass that path through to your source unless `--source` is exactly your id. See `_ingest` in `commands.py`.

## 3. Area coverage

Run a video from the fixture through `areas.affinity`. If every row scores `0`, recommend will hide the source. Fix the stored topics or the area rules, and add a unit test next to `tests/test_areas.py`.

## 4. Offline test

Add a fixture directory the test writes under `tmp_path` (json and markdown, no zip of the real dataset). Ingest with:

```python
run_cli(home, ["ingest", "--source", "<id>", "--path", str(fixture)])
```

Assert:

- exit 0 and `changed: true`
- the second call prints `changed: false`
- `recommend` or `show` sees a vid from the fixture
- `urlopen` was not called (the autouse fixture in `tests/conftest.py` already fails the test if it was)

Also assert a bad host or a path escape raises `AppError` if your loader accepts a path.

## 5. Docs

One paragraph in `AGENTS.md` under "How to run" if the agent must pass a new flag. An ADR if you change the schema or the default ingest set. Do not commit fetched payloads.
