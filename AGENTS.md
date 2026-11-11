# AGENTS.md

HackerTube is a CLI for coding agents. Another agent should be able to use it
and extend it from this file plus the skills in `.agents/skills/`.

The audience is an AI/ML engineer. The three focus areas are
`agentic-engineering`, `ai-engineering`, and `ai-research`. There is no web
viewer and there should not be one.

## Repo map

```
src/                     Cargo library + hackertube binary
  cli.rs                 clap. Unknown flags exit 2
  commands.rs            one function per command
  output.rs              TOON and JSON rendering, truncation, errors
  areas.rs               focus areas: lexical rules plus seed-embedding cosine
  db.rs                  SQLite catalog
  vecstore.rs            VectorIndex trait and sqlite-vec impl
  embed.rs               lazy bge-small-en-v1.5 (fastembed, no query prefix)
  ingest.rs              hackertube and aietalks sources
  insights.rs            deterministic extraction
  state.rs               ~/.hackertube/state.json
tests/                   cargo test. Offline (HACKERTUBE_EMBED=off)
docs/adr/                architecture decisions
.agents/skills/          contributor and usage skills
.cursor/skills, .claude/skills   symlinks to .agents/skills
```

Mutable state lives only under `HACKERTUBE_HOME` (default `~/.hackertube`):

| Path | What |
|---|---|
| `catalog.sqlite` | videos, mentions, related ids, transcript chunks, FTS, vec0 |
| `state.json` | seen / like / dislike / snooze. The agent owns this file |
| `transcripts/<vid>.md` | cached transcript markdown |
| `models/` | lazy ONNX download for bge-small-en-v1.5 |
| `cache/` | downloaded zip, AI Engineer pages |
| `hooks/session.sh` | written by `setup` |

`HACKERTUBE_TODAY=YYYY-MM-DD` freezes the clock for snooze expiry. It is a
supported replay switch, not a hidden test hook.

## Setup

```bash
cargo build --release
./target/release/hackertube --help
```

Stable Rust. Semantic search is on by default. The model downloads on first
use. Set `HACKERTUBE_EMBED=off` to force lexical search. If the model cannot
load, commands still succeed and print `semantic: off` plus `notice`.

## How to run

```bash
cargo build --release
./target/release/hackertube
./target/release/hackertube ingest --path /path/to/unzipped-or-zip
./target/release/hackertube recommend --area agentic-engineering
./target/release/hackertube search "context engineering"
./target/release/hackertube search "harness" --in transcripts
./target/release/hackertube similar <vid>
./target/release/hackertube show <vid>
./target/release/hackertube transcript <vid>
./target/release/hackertube insights <vid>
./target/release/hackertube seen <vid>
./target/release/hackertube snooze <vid> --days 7
./target/release/hackertube forget <vid>
./target/release/hackertube setup
```

`ingest` without `--path` downloads
`https://hackertube.org/download/hackertube-data.zip` and the full AI Engineer
transcript catalog. Do not commit that zip, model files, or a filled
`HACKERTUBE_HOME`. `--limit N` caps AI Engineer talks; `0` or omitted means all.

AI Engineer ingest:

1. `GET https://ai.engineer/api/data/transcripts?format=json`
2. Join `https://ai.engineer/api/data/talks?format=json` on `talkSlug` to get `videoId` and the talk URL. The transcript index has no video id.
3. `GET {url}.md`
4. If the markdown has `Source:` / `Published:` lines (AIE Talks write-ups), those win and must match the joined video id. Official ai.engineer markdown has a YouTube link instead; that link must match too.
5. Chunk the transcript (~800 characters) and embed each chunk into `vec_chunks` so `search --in transcripts` and video `similar` can use both sources.

Only `ai.engineer`, `aietalks.com`, and `hackertube.org` are fetched. A local
snapshot (`transcripts.json`, `talks.json`, `markdown/<vid>.md`) is what tests
use. Pass it with `--source aietalks --path <dir>`.

## How to test

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs those three. Tests set `HACKERTUBE_EMBED=off` and
`HACKERTUBE_CAPTIONS=off`. Do not mark a test as requiring the network. Build
a fixture directory and pass `--path`.

`HACKERTUBE_HOME` and `HOME` in CLI tests point at a temp directory so `setup`
cannot touch a real `~/.claude`.

## Conventions

- Imports at the top of the module. No inline imports.
- `match` on a closed enum ends in a `never` check when a new variant should fail the build.
  A new source or command is a new match arm, not a plugin registry loaded at runtime.
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

Follow `.agents/skills/add-source/SKILL.md`. Short version: add
`ingest_<id>` and a match arm in `ingest_source`, upsert videos through
`db::upsert_video` and vectors through `VectorIndex`, and add an offline
fixture test (`HACKERTUBE_EMBED=off`).

## How to add a command

Follow `.agents/skills/add-command/SKILL.md`. Register the clap variant in
`cli.rs` and the handler in `commands.rs`. Print through `output::render`.
Add an exit-code test and an output-contract assertion (header fields, `help[`,
empty state).

## Definition of done

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass.
- A new command or source has a fixture test with the network guard still on.
- List output still defaults to 3–4 fields and ends with `help`.
- An ADR exists if storage, ranking, or the output contract changed.
- README/NOTICE still attribute the ODC-By HackerTube dataset.

## What not to do

- Do not add a web UI, server, or HTML viewer.
- Do not commit the dataset zip, `videos.jsonl`, `embeddings.npy`, transcripts,
  or YouTube API payloads.
- Do not print errors only on stderr, prompt, or page.
- Do not call an LLM or caption hosts unless the matching env var is set and
  the user passed the flag (`--llm`) or a transcript is missing and
  `HACKERTUBE_CAPTIONS` is not `off`. Default `insights` stays deterministic.
  There is no YouTube Data API command.
- Do not shell out to the 10x-swe `distiller` binary. See
  `docs/adr/0005-insight-extraction.md`.
- Do not query `vec0` from outside `vecstore.rs`. See `docs/adr/0008-sqlite-vec.md`.
- Do not bring the Python package back.
- Do not widen default rows past four fields to "be helpful".
- Do not rank with a hidden model call or an unstated random sample.
- Do not put a second copy of a skill under `.cursor/` or `.claude/`. Symlink.
