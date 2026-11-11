---
name: add-command
description: Add a HackerTube CLI command that follows AXI and the output contract. Use when adding a subcommand, flag, or stdout field to hackertube.
---

# Add a command

Commands are agent calls. The grammar is `hackertube <verb> [vid|query] [--flags]`. No prompts. Stdout is the document. Exit 0, 1, or 2 as in `AGENTS.md`.

## 1. Parser

In `src/cli.rs`, add a clap variant. `--json`, `--full`, and `--fields` are global. One-line doc comment, which clap uses as `--help`.

Unknown flags become exit 2 with a structured stdout error. Do not read stdin.

## 2. Handler

Add a function in `src/commands.rs` and call it from `cli::dispatch`. Signature: `fn example(flags: &Flags, ...) -> Result<(Doc, i32), AppError>`. The function emits the document itself.

Build a `Doc`:

- scalars the agent needs (`vid`, `count`, `mode`) via `doc.add`
- one table per list, default 3 or 4 fields, via `output::project_fields` then `doc.table`
- `doc.help` with concrete next commands that include a real id from the result when you have one

```rust
let mut doc = Doc::new();
doc.add("count", format!("{} of {total}", records.len()));
doc.table("videos", &["vid", "title", "score", "reason"], rows);
if records.is_empty() {
    doc.add("empty", "no videos match the filters");
}
doc.help(&["hackertube show <vid>"]);
output::emit(&doc, flags.json, flags.full);
Ok((doc, 0))
```

Operational misses return `AppError::new("not_found", detail).with("vid", vid)` (exit 1). Bad arguments return `UsageError` (exit 2). Do not `print!` the document inside the handler; `output::emit` is the stdout path. Do not panic for a missing video.

If the command changes a file, write the same bytes on a repeat and return `changed: false` when the stored value already matches. Exit 0.

## 3. Fields and truncation

Default columns are the contract. Add any extra selectable names to the allow-list passed to `output::project_fields`. `--fields` replaces the default; an unknown name is exit 2.

Do not put a fifth column on the default row. Put the extra fact in a scalar (`count`, `mode`, `excluded`) instead.

`output::emit` truncates long strings unless `flags.full`. Do not pre-slice URLs.

## 4. Tests

In `tests/cli.rs`, using `assert_cmd` and a temp `HACKERTUBE_HOME`:

- success exit 0, stdout contains the table header `name[N]{col,col,col}:` and `help[`
- empty input or empty catalog exit 0 with `empty:` and a `[0]` header, not a panic
- repeat of a mutation: first `changed: true`, second `changed: false`
- `--json` parses and has a `help` array
- unknown flag exit 2, stdout contains `error:` and `exit: 2`
- a missing video exit 1 with `error: not_found` on stdout

Set `HACKERTUBE_EMBED=off` and `HACKERTUBE_CAPTIONS=off`. Do not download.

## 5. Docs

Add the verb to the command table in `README.md` and the "How to run" list in `AGENTS.md`. If the contract gained a new required field or exit meaning, add an ADR. Keep `--help` to a sentence plus the flags; do not paste a tutorial into the epilog.
