---
name: add-command
description: Add a HackerTube CLI command that follows AXI and the output contract. Use when adding a subcommand, flag, or stdout field to hackertube.
---

# Add a command

Commands are agent calls. The grammar is `hackertube <verb> [vid|query] [--flags]`. No prompts. Stdout is the document. Exit 0, 1, or 2 as in `AGENTS.md`.

## 1. Parser

In `src/hackertube/cli.py`, add a subparser with `parents=[common]` so `--json`, `--full`, and `--fields` work before or after the verb. Set `allow_abbrev` by using `Parser`. One-line `help=`.

```python
example = sub.add_parser("example", parents=[common], help="One line an agent can scan")
example.add_argument("vid")
example.add_argument("--limit", type=int)
```

Unknown flags must hit `Parser.error` and become exit 2. Do not set `allow_abbrev=True`. Do not read stdin unless the command's contract says so (none of the current commands do).

## 2. Handler

Add the verb to the dict in `commands.run`. The handler signature is `(args, flags, home) -> tuple[dict, int]`.

Build a flat dict:

- scalars the agent needs (`vid`, `count`, `mode`)
- one `Table` per list, default 3 or 4 fields, via `_table`
- `help` is added by `_finish`; pass concrete next commands that include a real id from the result when you have one

```python
document = {
    "count": f"{len(shown)} of {len(matched)}",
    "videos": _table(("vid", "title", "score", "reason"), rows, flags, VIDEO_FIELDS),
}
if not shown:
    document["empty"] = "no videos match the filters"
return _finish(document, flags, [f"hackertube show {vid}"]), 0
```

Operational misses raise `AppError("not_found", detail, vid=vid)` (exit 1). Bad arguments raise `UsageError` (exit 2). Do not `print()` inside the handler. Do not traceback.

If the command changes a file, write the same bytes on a repeat and return `changed: false` when the stored value already matches. Exit 0.

## 3. Fields and truncation

Default columns are the contract. Add any extra selectable names to the `allowed` set passed to `_table`. `--fields` replaces the default; an unknown name is exit 2.

Do not put a fifth column on the default row. Put the extra fact in a scalar (`count`, `mode`, `excluded`) instead.

`_finish` truncates long strings unless `flags.full`. Do not pre-slice URLs.

Insight-style tables that do not share columns with video rows should pass `honor_fields=False` so a video `--fields` list does not crash them.

## 4. Tests

In `tests/test_cli.py` (or a new file that uses `run_cli`):

- success exit 0, stdout contains the table header `name[N]{col,col,col}:` and `help[`
- empty input or empty catalog exit 0 with `empty:` and a `[0]` header, not a stack trace
- repeat of a mutation: first `changed: true`, second `changed: false`
- `--json` parses with `json.loads` and has a `help` array
- unknown flag exit 2, stdout contains `error:` and `exit: 2`
- a missing video exit 1 with `error: not_found` (or your code) on stdout

Use `write_dataset` / `write_aietalks`. Do not download.

## 5. Docs

Add the verb to the command table in `README.md` and the "How to run" list in `AGENTS.md`. If the contract gained a new required field or exit meaning, add an ADR. Keep `--help` to a sentence plus the flags; do not paste a tutorial into the epilog.
