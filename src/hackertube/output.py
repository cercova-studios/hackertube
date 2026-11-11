"""AXI output: TOON by default, JSON when asked, truncation, structured errors.

List rows stay at 3–4 fields unless the caller passes ``--fields``. Long text is
cut with a ``…[+Nc]`` hint unless ``full`` is set. Every document ends with a
``help`` array of next-step command templates.
"""

from __future__ import annotations

import json
import math
import re
import sys
from collections.abc import Mapping, Sequence
from numbers import Real

from hackertube.errors import UsageError

_CELL_LIMIT = 110
_TEXT_LIMIT = 180
_NO_TRUNCATE = {"vid", "url", "page", "source_url", "bin", "home", "catalog", "state_file", "flag"}
_BARE_KEY = re.compile(r"^[A-Za-z_][A-Za-z0-9_-]*$")
_NUMERIC = re.compile(r"^-?\d+(\.\d+)?$")


class Table:
    """Uniform rows. Field order is the on-wire column order, including when empty."""

    def __init__(self, fields: Sequence[str], rows: Sequence[Mapping[str, object]]) -> None:
        self.fields = tuple(fields)
        self.rows = [dict(row) for row in rows]

    def as_json(self) -> list[dict[str, object]]:
        return [{field: row.get(field) for field in self.fields} for row in self.rows]


def truncate(value: str, limit: int) -> str:
    if len(value) <= limit:
        return value
    marker_budget = 14
    cut = value[: max(1, limit - marker_budget)].rstrip()
    omitted = len(value) - len(cut)
    return f"{cut}…[+{omitted}c]"


def apply_fields(
    rows: Sequence[Mapping[str, object]],
    default: Sequence[str],
    requested: Sequence[str] | None,
    allowed: set[str],
) -> Table:
    fields = tuple(requested) if requested else tuple(default)
    if not fields:
        raise UsageError("--fields needs at least one name")
    unknown = [field for field in fields if field not in allowed]
    if unknown:
        raise UsageError(f"unknown field {unknown[0]}", flag="--fields")
    projected = [{field: row.get(field) for field in fields} for row in rows]
    return Table(fields, projected)


def shorten_rows(table: Table, *, full: bool) -> Table:
    if full:
        return table
    rows: list[dict[str, object]] = []
    for row in table.rows:
        short: dict[str, object] = {}
        for field, value in row.items():
            if isinstance(value, str) and field not in _NO_TRUNCATE:
                limit = (
                    _TEXT_LIMIT if field in {"text", "summary", "why", "reason"} else _CELL_LIMIT
                )
                short[field] = truncate(value, limit)
            else:
                short[field] = value
        rows.append(short)
    return Table(table.fields, rows)


def shorten_scalars(data: dict[str, object], *, full: bool) -> dict[str, object]:
    if full:
        return data
    out: dict[str, object] = {}
    for key, value in data.items():
        if isinstance(value, str) and key not in _NO_TRUNCATE and len(value) > _TEXT_LIMIT:
            out[key] = truncate(value, _TEXT_LIMIT)
        elif isinstance(value, Table):
            out[key] = shorten_rows(value, full=False)
        else:
            out[key] = value
    return out


def render(data: Mapping[str, object], *, as_json: bool) -> str:
    if as_json:
        return json.dumps(_jsonable(data), ensure_ascii=False, indent=2) + "\n"
    return _to_toon(data) + "\n"


def emit(data: Mapping[str, object], *, as_json: bool) -> None:
    sys.stdout.write(render(data, as_json=as_json))


def error_document(
    code: str,
    detail: str,
    exit_code: int,
    help_lines: Sequence[str],
    **extra: object,
) -> dict[str, object]:
    document: dict[str, object] = {
        "ok": False,
        "error": code,
        "detail": detail,
        "exit": exit_code,
    }
    for key, value in extra.items():
        if value is not None:
            document[key] = value
    document["help"] = list(help_lines) or ["hackertube --help"]
    return document


def _jsonable(value: object) -> object:
    if isinstance(value, Table):
        return value.as_json()
    if isinstance(value, Mapping):
        return {str(key): _jsonable(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_jsonable(item) for item in value]
    return value


def _to_toon(data: Mapping[str, object]) -> str:
    lines: list[str] = []
    for key, value in data.items():
        lines.extend(_encode(str(key), value, depth=0))
    return "\n".join(lines)


def _encode(key: str, value: object, depth: int) -> list[str]:
    indent = "  " * depth
    name = _quote_key(key)
    if isinstance(value, Table):
        header = f"{indent}{name}[{len(value.rows)}]{{{','.join(value.fields)}}}:"
        rows = [header]
        for row in value.rows:
            cells = [_quote_cell(row.get(field), ",") for field in value.fields]
            rows.append(f"{indent}  {','.join(cells)}")
        return rows
    if isinstance(value, list):
        cells = [_quote_cell(item, ",") for item in value]
        return [f"{indent}{name}[{len(value)}]: {','.join(cells)}".rstrip()]
    return [f"{indent}{name}: {_scalar(value)}"]


def _quote_key(key: str) -> str:
    if _BARE_KEY.fullmatch(key):
        return key
    return _quote_cell(key, ",")


def _scalar(value: object) -> str:
    if isinstance(value, str):
        if value == "" or "\n" in value or value != value.strip() or value[:1] in "{[":
            return _quote_cell(value, ",")
        return value
    return _quote_cell(value, ",")


def _quote_cell(value: object, delimiter: str) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, Real):
        return _format_number(float(value))
    text = str(value)
    if (
        text == ""
        or text != text.strip()
        or any(char in text for char in (delimiter, '"', "\\", "\n", "\r", "\t"))
        or text.lower() in {"true", "false", "null"}
        or _NUMERIC.fullmatch(text) is not None
    ):
        escaped = (
            text.replace("\\", "\\\\")
            .replace('"', '\\"')
            .replace("\n", "\\n")
            .replace("\r", "\\r")
            .replace("\t", "\\t")
        )
        return f'"{escaped}"'
    return text


def _format_number(value: float) -> str:
    if not math.isfinite(value):
        return "null"
    if abs(value - round(value)) < 1e-9 and abs(value) < 1e15:
        return str(int(round(value)))
    text = f"{value:.4f}".rstrip("0").rstrip(".")
    return text or "0"
