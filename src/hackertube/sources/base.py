"""Shared ingest types. Sources must not prompt, and they must be safe to re-run."""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from pathlib import Path


@dataclass
class IngestContext:
    conn: sqlite3.Connection
    home: Path
    path: Path | None
    limit: int | None
    refresh: bool


@dataclass(frozen=True)
class SourceResult:
    source: str
    videos: int
    embedded: int
    changed: bool
    detail: str
