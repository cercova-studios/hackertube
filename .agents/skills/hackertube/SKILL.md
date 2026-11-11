---
name: hackertube
description: Use the hackertube CLI to recommend videos and extract grounded insights for an AI/ML engineer. Use when the user wants videos on agentic engineering, AI engineering, or AI research, or a transcript, chapters, claims, tools, papers, or quotes from a talk.
---

# Using HackerTube

`hackertube` is the interface. Do not scrape YouTube or hackertube.org yourself when this CLI is installed. Do not build a UI around it.

Install once if the command is missing:

```bash
cargo build --release
```

The binary is `target/release/hackertube`. The first search or ingest that needs vectors downloads bge-small-en-v1.5 into `~/.hackertube/models`. `semantic: off` plus a `notice` means that download failed and results are keyword-only.

`HACKERTUBE_HOME` defaults to `~/.hackertube`. State (`state.json`) is yours. Do not commit it.

## First call

```bash
hackertube
```

No arguments prints the home view: bin path, catalog path, state counts, one pick per focus area, and `help[]` lines. If it says `empty: no catalog yet`, ingest:

```bash
hackertube ingest --path /path/to/hackertube-data
hackertube ingest --source aietalks --limit 20
```

`--limit 0` on `aietalks` pulls the full AI Engineer index. The default 20 is intentional. Omit `--path` on the dataset source only when a download is acceptable; the URL is `https://hackertube.org/download/hackertube-data.zip`.

## Recommend

```bash
hackertube recommend --area agentic-engineering
hackertube recommend --area ai-engineering
hackertube recommend --area ai-research
```

Rows are `vid, title, score, reason`. The reason is copied from the catalog, not written by you. Pass a row to the user with the vid and the reason. Do not rewrite the reason into a stronger claim.

The list already hides seen, disliked, and unexpired snoozes, plus news. Likes remain in the list. After the user watches one:

```bash
hackertube seen <vid>
```

Not interested: `hackertube dislike <vid>`. Later: `hackertube snooze <vid> --days 7`. Undo: `hackertube forget <vid>`. Repeating a mark is safe (`changed: false`, exit 0).

`--include-news` keeps shelf-life `news`. `--json` when you need to parse. `--fields vid,title,score` to drop the reason. `--full` when a reason ends in `…[+Nc]`.

## Search and neighbors

```bash
hackertube search "mcp harness"
hackertube search "graded on completed work" --in transcripts
hackertube similar <vid>
hackertube show <vid>
```

`semantic: off` plus a `notice` means the embedding model could not run; results are keyword only. Say that if you relied on meaning rather than words. Transcript hits include `at` and the vid. Open that timestamp; do not guess one.

## Transcript and insights

```bash
hackertube transcript <vid>
hackertube insights <vid>
```

AI Engineer markdown is preferred. Otherwise the command scrapes YouTube caption tracks. `HACKERTUBE_CAPTIONS=off` skips that network call. Exit 1 and `error: transcript_unavailable` means there is no text. Do not invent quotes.

`insights` returns `chapters`, `claims`, `mentions` (`kind` is `tool`, `paper`, or `repo`), and `quotes`. Every item has `at` and `url`. Quote `text` and cite `url`. If `at` is empty, the name appeared in the preamble and has no timestamp; do not attach one.

`hackertube insights <vid> --llm` is optional and only works when `HACKERTUBE_LLM_API_KEY` is set. Without the flag the command is deterministic. Prefer the default. If you use `--llm`, still only repeat claims that appear in the output. Dropped paraphrases never show up.

## Parse the document

TOON tables:

```
videos[2]{vid,title,score,reason}:
  aaaaaaaaaaa,"Let's build GPT",10,HN's favorite GPT walkthrough.
```

`count:` is `shown of eligible`. `empty:` means stop, do not retry the same query. `ok: false` with `exit: 1` is a real failure. Exit 2 is a bad flag; read `hackertube <command> --help` (it is short) and fix the invocation.

## Session hook

`hackertube setup` installs a Claude Code `SessionStart` hook that runs `hackertube`, plus `~/.hackertube/hooks/session.sh`. Run it once if this session should see fresh picks automatically. It is idempotent.

## Do not

- Do not recommend a vid the command excluded.
- Do not call the YouTube Data API. The CLI does not. Use `show` and `transcript` for what is stored locally.
- Do not paste the dataset zip into the repo.
