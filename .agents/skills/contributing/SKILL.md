---
name: contributing
description: Contribute to the HackerTube CLI. Use when changing hackertube source, tests, docs, or CI. Covers branch, commit, test, and PR conventions and the checks that must pass.
---

# Contributing to HackerTube

HackerTube is an agent-first CLI. Read `AGENTS.md` before editing. Do the work from this skill without asking for a walkthrough.

## Workflow

1. Branch from `main`. Name: `cursor/<short-topic>-<id>` when the cloud agent requires that shape; otherwise a short lowercase hyphenated name.
2. Change the smallest surface that solves the task. Ranking, storage, or the output contract also needs an ADR in `docs/adr/`.
3. Run the checks below. Fix failures. Do not delete a test to go green.
4. Commit with a sentence that says what changed and why.
5. Open a PR against `main`. The description states the user-visible behavior. If the CLI output changed, paste real stdout of the affected command.

## Checks

From the repo root, with dev extras installed (`uv sync --extra dev`):

```bash
uv run ruff check .
uv run ruff format --check .
uv run mypy src
uv run pytest
```

CI runs the same four commands (`.github/workflows/ci.yml`). All four must pass.

## Rules that are easy to break

- Tests must not call the network. `tests/conftest.py` replaces `urllib.request.urlopen`. Build a directory fixture and pass `--path`.
- Do not commit the HackerTube zip, `videos.jsonl`, `embeddings.npy`, transcripts, or YouTube API cache.
- Keep ODC-By attribution in `README.md` and `NOTICE` (HackerTube and https://hackertube.org).
- Default list rows stay at 3–4 fields. Errors go to stdout. Mutations stay idempotent (`changed: false` on a repeat).
- New skills go in `.agents/skills/<name>/SKILL.md` and are symlinked from `.cursor/skills/` and `.claude/skills/`. Do not paste a second copy.
- Imports stay at the top of the file. Optional dependencies are a top-level `try/except ImportError`.
- No web UI.

## PR checklist

- [ ] `ruff`, `mypy`, and `pytest` are green.
- [ ] New command or source has a fixture test and an output-contract assertion (`help[`, field list, empty state, exit code).
- [ ] ADR added if the contract, schema, or focus areas changed.
- [ ] No dataset dump in the diff (`git status` should not show `*.zip` or `embeddings.npy`).
