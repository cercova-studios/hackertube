# HackerTube

Agent-first CLI that ranks videos for an AI/ML engineer and pulls grounded
insights out of them. The users are coding agents. There is no web UI.

Focus areas: `agentic-engineering`, `ai-engineering`, `ai-research`.

```bash
uv sync --extra dev --extra embed
uv run hackertube ingest --path /path/to/hackertube-data
uv run hackertube
uv run hackertube recommend --area agentic-engineering
uv run hackertube insights <vid>
```

`ingest` with no `--path` downloads the dataset from
https://hackertube.org/download/hackertube-data.zip into `HACKERTUBE_HOME`
(default `~/.hackertube`). The zip is not part of this repository.
`--source aietalks` reads https://ai.engineer/api/data/transcripts?format=json,
joins YouTube ids from the talks index, and caches each talk's markdown.

Output is [TOON](https://toonformat.dev/) by default. `--json` switches to JSON.
Read [AGENTS.md](AGENTS.md) before changing the CLI. Decisions live in
[docs/adr](docs/adr).

## Data attribution

The HackerTube catalog is © its contributors and released under the
[Open Data Commons Attribution License (ODC-By) v1.0](https://opendatacommons.org/licenses/by/1-0/).
Credit **HackerTube** (https://hackertube.org) and the
[open-index/hacker-news](https://huggingface.co/datasets/open-index/hacker-news) dataset.
Hacker News data is provided by Y Combinator. Video titles and channel names
belong to their creators. Not affiliated with YouTube, Google, Hacker News,
or Y Combinator. See [NOTICE](NOTICE).

AI Engineer transcripts stay on disk under `HACKERTUBE_HOME` and are not
committed here.

## Commands

| Command | What an agent gets |
|---|---|
| `hackertube` | Home view: bin path, counts, one pick per focus area |
| `ingest` | Load a source. Idempotent |
| `recommend --area <id>` | Ranked unseen videos with a one-line reason copied from stored text |
| `search <query>` | Keyword search, plus cosine similarity when fastembed is installed |
| `search <query> --in transcripts` | Timestamped transcript hits |
| `similar <vid>` | Embedding neighbors, else co-recommended videos |
| `show <vid>` | One card |
| `transcript <vid>` | Cached AI Engineer markdown, else YouTube captions |
| `insights <vid>` | Chapters, claims, tools/papers/repos, quotes. Each item has a timestamp and URL |
| `seen` / `like` / `dislike` / `snooze` / `forget` | Marks in `~/.hackertube/state.json` |
| `setup` | Optional session hook |

Likes stay recommendable. Seen, disliked, and unexpired snoozes do not.
`HACKERTUBE_LLM_API_KEY` plus `--llm` is the only path that calls a model.
`YOUTUBE_API_KEY` plus `enrich` is the only path that calls the YouTube Data API.
