# HackerTube

Agent-first CLI that ranks videos for an AI/ML engineer and pulls grounded
insights out of them. The users are coding agents. There is no web UI.

Focus areas: `agentic-engineering`, `ai-engineering`, `ai-research`.

```bash
cargo build --release
./target/release/hackertube ingest --path /path/to/hackertube-data
./target/release/hackertube
./target/release/hackertube recommend --area agentic-engineering
./target/release/hackertube insights <vid>
```

`ingest` with no `--path` downloads the dataset from
https://hackertube.org/download/hackertube-data.zip into `HACKERTUBE_HOME`
(default `~/.hackertube`). The zip is not part of this repository.
`ingest` also loads the full AI Engineer transcript catalog
(https://ai.engineer/api/data/transcripts?format=json), joins YouTube ids from
the talks index, caches each talk's markdown (`url` + `.md`), and embeds
transcript chunks with bge-small-en-v1.5. `--limit N` caps that catalog; `0`
means all. The ONNX model downloads lazily into `HACKERTUBE_HOME/models`. If
it cannot load, search falls back to keywords and says so.

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
| `search <query>` | Keyword search blended with cosine similarity. `semantic: off` when the model cannot run |
| `search <query> --in transcripts` | Timestamped transcript hits |
| `similar <vid>` | Embedding neighbors, else co-recommended videos |
| `show <vid>` | One card |
| `transcript <vid>` | Cached AI Engineer markdown, else YouTube captions |
| `insights <vid>` | Chapters, claims, tools/papers/repos, quotes. Each item has a timestamp and URL |
| `seen` / `like` / `dislike` / `snooze` / `forget` | Marks in `~/.hackertube/state.json` |
| `setup` | Optional session hook |

Likes stay recommendable. Seen, disliked, and unexpired snoozes do not.
`HACKERTUBE_LLM_API_KEY` plus `--llm` is the only path that calls a chat model.
Caption fallback reads the public watch page; there is no YouTube Data API command.
