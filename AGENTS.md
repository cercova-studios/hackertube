# AGENTS.md

HackerTube is a CLI for coding agents. Another agent should be able to use it
and extend it from this file plus the skills in `.agents/skills/`.

The audience is an AI/ML engineer. The three focus areas are
`agentic-engineering`, `ai-engineering`, and `ai-research`. There is no web
viewer and there should not be one.

## Repo map

```
src/hackertube/          CLI package. Console script: hackertube
  cli.py                 argparse. Unknown flags exit 2
  commands.py            one function per command
  output.py              TOON and JSON rendering, truncation, errors
  areas.py               focus-area rules (data, not a model)
  rank.py                affinity, then score, then vid
  search.py              FTS5, optionally blended with cosine similarity
  db.py                  SQLite catalog
  embed.py               embeddings.npy, 384-d, L2-normalized
  state.py               ~/.hackertube/state.json
  sources/               pluggable ingest. Register in SOURCES
  insights.py            deterministic extraction
  transcript.py          markdown identity + caption fallback
  youtube_api.py         optional Data API cache
  session.py             opt-in session hook
tests/                   pytest. No network (urlopen is blocked)
docs/adr/                architecture decisions
.agents/skills/          contributor and usage skills
.cursor/skills, .claude/skills   symlinks to .agents/skills
```

Mutable state lives only under `HACKERTUBE_HOME` (default `~/.hackertube`):

| Path | What |
|---|---|
| `catalog.sqlite` | videos, mentions, related ids, transcript segments, FTS |
| `embeddings.npy` | float16 matrix, row `i` via `embed_rows` |
| `state.json` | seen / like / dislike / snooze. The agent owns this file |
| `transcripts/<vid>.md` | cached transcript markdown |
| `cache/` | downloaded zip, AI Engineer pages, YouTube metadata |
| `hooks/session.sh` | written by `setup` |

`HACKERTUBE_TODAY=YYYY-MM-DD` freezes the clock for snooze expiry. It is a
supported replay switch, not a hidden test hook.

## Setup

```bash
uv sync --extra dev          # pytest, ruff, mypy
uv sync --extra embed        # fastembed, for query and AI Engineer vectors
uv sync --extra captions     # youtube-transcript-api
uv run hackertube --help
```

Python 3.11+. Core install needs only numpy. Semantic search and AI Engineer
embeddings no-op (and say `semantic: off` / `embedded: 0`) until `embed` is
installed. Caption fallback no-ops with `transcript_unavailable` until
`captions` is installed. Neither missing extra is allowed to traceback.

## How to run

```bash
uv run hackertube
uv run hackertube ingest --path /path/to/unzipped-or-zip
uv run hackertube ingest --source aietalks --limit 20   # 0 = full index
uv run hackertube recommend --area agentic-engineering
uv run hackertube search "coding agents"
uv run hackertube search "harness" --in transcripts
uv run hackertube similar <vid>
uv run hackertube show <vid>
uv run hackertube transcript <vid>
uv run hackertube insights <vid>
uv run hackertube seen <vid>
uv run hackertube snooze <vid> --days 7
uv run hackertube forget <vid>
uv run hackertube setup
```

`ingest` without `--path` downloads
`https://hackertube.org/download/hackertube-data.zip`. Do not commit that zip,
`embeddings.npy`, or a filled `HACKERTUBE_HOME`.

AI Engineer ingest:

1. `GET https://ai.engineer/api/data/transcripts?format=json`
2. Join `https://ai.engineer/api/data/talks?format=json` on `talkSlug` to get `videoId` and the talk URL. The transcript index has no video id.
3. `GET {url}.md`
4. If the markdown has `Source:` / `Published:` lines (AIE Talks write-ups), those win and must match the joined video id. Official ai.engineer markdown has a YouTube link instead; that link must match too.

Only `ai.engineer` and `aietalks.com` are fetched. A local snapshot
(`transcripts.json`, `talks.json`, `markdown/<vid>.md`) is what tests use.
Pass it with `--source aietalks --path <dir>`.

## How to test

```bash
uv run ruff check .
uv run ruff format --check .
uv run mypy src
uv run pytest
```

CI runs those four. Tests refuse `urllib.request.urlopen`. Do not skip that
guard. Do not mark a test as requiring the network. Build a fixture directory
and pass `--path`.

`HACKERTUBE_HOME` and `HOME` in CLI tests point at `tmp_path` so `setup` cannot
touch a real `~/.claude`.

## Conventions

- Imports at the top of the module. Optional packages (`fastembed`,
  `youtube-transcript-api`) use a top-level `try/except ImportError`.
- `switch` on a closed union gets a `never` default. This codebase is small;
  prefer a dict dispatch that a new source or command must be added to.
- Ranking, extraction, and rendering are pure functions of their inputs.
  Do not call an LLM from those functions. `insights --llm` is the only model
  call, and only when `HACKERTUBE_LLM_API_KEY` is set.
- New behavior that changes command shape, storage, or a focus area gets a
  short ADR in `docs/adr/` that says why the choice is better for an agent.
- Skills ship in `.agents/skills/<name>/SKILL.md` with `name` and `description`
  frontmatter. `.cursor/skills/<name>` and `.claude/skills/<name>` are relative
  symlinks to that directory. Do not copy the skill body into a second place.
- Default list rows have 3 or 4 fields. `--fields` may ask for more. Insight
  tables stay on their own columns and ignore `--fields` because chapters and
  mentions do not share a schema.
- Reasons are copied from `why`, else the first sentence of `summary`, else
  the stored topic list. Never invent a reason.
- Every extracted item carries `at` (timestamp, possibly empty) and `url`
  (YouTube URL, with `&t=` when the timestamp is known).

## Output contract

Success and failure both print a document on **stdout**. stderr stays empty
unless `HACKERTUBE_DEBUG=1` (not required). Exit `0` success, including empty
results and idempotent no-ops. Exit `1` operational failure (`not_found`,
download, transcript, LLM). Exit `2` usage (`unknown_flag`, bad area, bad vid).

TOON is the default. `--json` is the same document as JSON.

List header: `videos[5]{vid,title,score,reason}:` then one row per line,
indented two spaces. Cells that contain the comma delimiter, quotes, or
newlines are quoted. `help[N]:` is the last key and lists next commands.

Scalars: `key: value`. Booleans are `true` / `false`. Whole-number floats
render as integers (`10`, not `10.0`).

Empty lists still declare columns: `videos[0]{vid,title,score,reason}:` plus
`empty: ...` that says what was asked and that nothing matched. `count:` is
precomputed (`5 of 40 eligible`, `seen 1, disliked 0, snoozed 0`).

Truncation: long text becomes `prefix…[+Nc]` where N is the omitted count.
`--full` disables it. URLs, video ids, and paths are not truncated.

Errors:

```
ok: false
error: not_found
detail: no video zzzzzzzzzzz
vid: zzzzzzzzzzz
exit: 1
help[1]: hackertube --help
```

Mutations (`seen`, `like`, `dislike`, `snooze`, `forget`, `ingest`, `setup`)
are safe to repeat. The second call returns `changed: false` and exit 0 when
the stored mark or source fingerprint is already what was asked. `ingest`
does not touch `state.json`.

`recommend` drops `seen`, `disliked`, and snoozes whose `until` is today or
later. `liked` does not drop a video. Shelf life `news` is hidden unless
`--include-news`. HackerTube rows with zero recommenders are hidden. Comedy
and music are out of every focus area.

No interactive prompts. No `input()`. No pager.

## How to add a source

Follow `.agents/skills/add-source/SKILL.md`. Short version: add a module under
`src/hackertube/sources/`, register it in `SOURCES`, accept `IngestContext`,
return `SourceResult`, upsert videos, and add an offline fixture test that
fails if `urlopen` is called.

## How to add a command

Follow `.agents/skills/add-command/SKILL.md`. Register the subparser in
`cli.py` and the handler in `commands.run`. Print through `output.render`.
Add an exit-code test and an output-contract assertion (header fields, `help[`,
empty state).

## Definition of done

- `ruff check`, `ruff format --check`, `mypy src`, and `pytest` pass.
- A new command or source has a fixture test with the network guard still on.
- List output still defaults to 3–4 fields and ends with `help`.
- An ADR exists if storage, ranking, or the output contract changed.
- README/NOTICE still attribute the ODC-By HackerTube dataset.

## What not to do

- Do not add a web UI, server, or HTML viewer.
- Do not commit the dataset zip, `videos.jsonl`, `embeddings.npy`, transcripts,
  or YouTube API payloads.
- Do not print errors only on stderr, prompt, or page.
- Do not call an LLM, the YouTube Data API, or caption hosts unless the
  matching env var is set and the user passed the flag (`--llm`, `enrich`,
  or a transcript miss). Default `insights` stays deterministic.
- Do not shell out to the 10x-swe `distiller` binary. See
  `docs/adr/0005-insight-extraction.md`.
- Do not widen default rows past four fields to "be helpful".
- Do not rank with a hidden model call or an unstated random sample.
- Do not put a second copy of a skill under `.cursor/` or `.claude/`. Symlink.
