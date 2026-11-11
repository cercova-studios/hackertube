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

From the repo root, with stable Rust (rustfmt and clippy installed):

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs those three commands (`.github/workflows/ci.yml`). All three must pass.

## Rules that are easy to break

- Tests must not call the network. `tests/cli.rs` sets `HACKERTUBE_EMBED=off` and `HACKERTUBE_CAPTIONS=off`. Build a directory fixture and pass `--path`.
- Do not commit the HackerTube zip, `videos.jsonl`, `embeddings.npy`, transcripts, the ONNX model, or YouTube API cache.
- Keep ODC-By attribution in `README.md` and `NOTICE` (HackerTube and https://hackertube.org).
- Default list rows stay at 3–4 fields. Errors go to stdout. Mutations stay idempotent (`changed: false` on a repeat).
- New skills go in `.agents/skills/<name>/SKILL.md` and are symlinked from `.cursor/skills/` and `.claude/skills/`. Do not paste a second copy.
- Vector search goes through `vecstore::VectorIndex`. Do not query `vec0` from a command.
- No web UI. Do not add a second implementation beside the Cargo binary.

## PR checklist

- [ ] `cargo fmt`, `clippy -D warnings`, and `cargo test` are green.
- [ ] New command or source has a fixture test and an output-contract assertion (`help[`, field list, empty state, exit code).
- [ ] ADR added if the contract, schema, or focus areas changed.
- [ ] No dataset dump in the diff (`git status` should not show `*.zip` or `embeddings.npy`).
