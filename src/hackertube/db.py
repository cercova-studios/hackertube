"""SQLite catalog. Embeddings stay in embeddings.npy; this file points at rows."""

from __future__ import annotations

import json
import sqlite3
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from hackertube.errors import AppError
from hackertube.models import Video, video_from_row

SCHEMA_VERSION = "1"

SCHEMA = """
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS videos (
  vid TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  url TEXT NOT NULL,
  page_url TEXT NOT NULL DEFAULT '',
  title TEXT NOT NULL,
  channel TEXT NOT NULL DEFAULT '',
  summary TEXT NOT NULL DEFAULT '',
  why TEXT NOT NULL DEFAULT '',
  content_type TEXT NOT NULL DEFAULT '',
  level TEXT NOT NULL DEFAULT '',
  shelf_life TEXT NOT NULL DEFAULT '',
  primary_topic TEXT NOT NULL DEFAULT '',
  tags_json TEXT NOT NULL DEFAULT '[]',
  categories_json TEXT NOT NULL DEFAULT '[]',
  topics_json TEXT NOT NULL DEFAULT '[]',
  quote TEXT NOT NULL DEFAULT '',
  hn_recommenders INTEGER NOT NULL DEFAULT 0,
  score REAL NOT NULL DEFAULT 0,
  published TEXT NOT NULL DEFAULT '',
  event TEXT NOT NULL DEFAULT '',
  transcript_path TEXT NOT NULL DEFAULT '',
  embed_row INTEGER,
  updated_at TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_videos_source ON videos(source);
CREATE TABLE IF NOT EXISTS mentions (
  hn_comment_id INTEGER NOT NULL,
  vid TEXT NOT NULL,
  hn_url TEXT NOT NULL DEFAULT '',
  stance TEXT NOT NULL DEFAULT '',
  why TEXT NOT NULL DEFAULT '',
  quote TEXT NOT NULL DEFAULT '',
  month TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (hn_comment_id, vid)
);
CREATE TABLE IF NOT EXISTS related (
  vid TEXT NOT NULL,
  other_vid TEXT NOT NULL,
  shared INTEGER NOT NULL,
  PRIMARY KEY (vid, other_vid)
);
CREATE TABLE IF NOT EXISTS embed_rows (
  vid TEXT PRIMARY KEY,
  row_index INTEGER NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS segments (
  vid TEXT NOT NULL,
  idx INTEGER NOT NULL,
  start_s INTEGER NOT NULL,
  text TEXT NOT NULL,
  url TEXT NOT NULL,
  PRIMARY KEY (vid, idx)
);
CREATE VIRTUAL TABLE IF NOT EXISTS videos_fts USING fts5(
  vid, title, channel, summary, why, topic, tags
);
CREATE VIRTUAL TABLE IF NOT EXISTS segments_fts USING fts5(
  vid, text, idx UNINDEXED
);
"""


def catalog_path(home: Path) -> Path:
    return home / "catalog.sqlite"


def connect(home: Path, *, create: bool = False) -> sqlite3.Connection:
    path = catalog_path(home)
    if not path.exists() and not create:
        raise AppError("catalog_missing", f"no catalog at {path}", path=str(path))
    if create:
        home.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys = ON")
    if create:
        init(conn)
    else:
        version = meta_get(conn, "schema_version")
        if version not in {None, SCHEMA_VERSION}:
            raise AppError(
                "schema_mismatch",
                f"catalog schema {version} is not {SCHEMA_VERSION}; delete the catalog and ingest",
            )
    return conn


def init(conn: sqlite3.Connection) -> None:
    conn.executescript(SCHEMA)
    meta_set(conn, "schema_version", SCHEMA_VERSION)
    conn.commit()


def meta_get(conn: sqlite3.Connection, key: str) -> str | None:
    row = conn.execute("SELECT value FROM meta WHERE key = ?", (key,)).fetchone()
    if row is None:
        return None
    return str(row["value"])


def meta_set(conn: sqlite3.Connection, key: str, value: str) -> None:
    conn.execute(
        "INSERT INTO meta(key, value) VALUES(?, ?) "
        "ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (key, value),
    )


@contextmanager
def session(home: Path, *, create: bool = False) -> Iterator[sqlite3.Connection]:
    conn = connect(home, create=create)
    try:
        yield conn
        conn.commit()
    except Exception:
        conn.rollback()
        raise
    finally:
        conn.close()


def catalog_exists(home: Path) -> bool:
    return catalog_path(home).exists()


def upsert_video(conn: sqlite3.Connection, record: dict[str, object]) -> None:
    columns = (
        "vid",
        "source",
        "url",
        "page_url",
        "title",
        "channel",
        "summary",
        "why",
        "content_type",
        "level",
        "shelf_life",
        "primary_topic",
        "tags_json",
        "categories_json",
        "topics_json",
        "quote",
        "hn_recommenders",
        "score",
        "published",
        "event",
        "transcript_path",
        "embed_row",
        "updated_at",
    )
    placeholders = ", ".join("?" for _ in columns)
    updates = ", ".join(f"{column}=excluded.{column}" for column in columns if column != "vid")
    values = [record.get(column, "") for column in columns]
    conn.execute(
        f"INSERT INTO videos({', '.join(columns)}) VALUES({placeholders}) "
        f"ON CONFLICT(vid) DO UPDATE SET {updates}",
        values,
    )


def replace_mentions(
    conn: sqlite3.Connection, source_vids: list[str], rows: list[tuple[object, ...]]
) -> None:
    if source_vids:
        conn.executemany("DELETE FROM mentions WHERE vid = ?", [(vid,) for vid in source_vids])
    conn.executemany(
        "INSERT OR REPLACE INTO mentions(hn_comment_id, vid, hn_url, stance, why, quote, month) "
        "VALUES(?, ?, ?, ?, ?, ?, ?)",
        rows,
    )


def replace_related(conn: sqlite3.Connection, rows: list[tuple[object, ...]]) -> None:
    conn.executemany(
        "INSERT OR REPLACE INTO related(vid, other_vid, shared) VALUES(?, ?, ?)",
        rows,
    )


def set_embed_row(conn: sqlite3.Connection, vid: str, row_index: int) -> None:
    conn.execute(
        "INSERT INTO embed_rows(vid, row_index) VALUES(?, ?) "
        "ON CONFLICT(vid) DO UPDATE SET row_index = excluded.row_index",
        (vid, row_index),
    )
    conn.execute("UPDATE videos SET embed_row = ? WHERE vid = ?", (row_index, vid))


def embed_map(conn: sqlite3.Connection) -> dict[str, int]:
    rows = conn.execute("SELECT vid, row_index FROM embed_rows").fetchall()
    return {str(row["vid"]): int(row["row_index"]) for row in rows}


def replace_segments(
    conn: sqlite3.Connection, vid: str, segments: list[tuple[int, int, str, str]]
) -> None:
    conn.execute("DELETE FROM segments WHERE vid = ?", (vid,))
    conn.executemany(
        "INSERT INTO segments(vid, idx, start_s, text, url) VALUES(?, ?, ?, ?, ?)",
        [(vid, idx, start_s, text, url) for idx, start_s, text, url in segments],
    )


def set_transcript_path(conn: sqlite3.Connection, vid: str, relative: str) -> None:
    conn.execute("UPDATE videos SET transcript_path = ? WHERE vid = ?", (relative, vid))


def rebuild_fts(conn: sqlite3.Connection) -> None:
    conn.execute("DELETE FROM videos_fts")
    rows = conn.execute(
        "SELECT vid, title, channel, summary, why, primary_topic, tags_json FROM videos"
    ).fetchall()
    payload = []
    for row in rows:
        tags = " ".join(json.loads(row["tags_json"] or "[]"))
        payload.append(
            (
                row["vid"],
                row["title"],
                row["channel"],
                row["summary"],
                row["why"],
                row["primary_topic"],
                tags,
            )
        )
    conn.executemany(
        "INSERT INTO videos_fts(vid, title, channel, summary, why, topic, tags) "
        "VALUES(?, ?, ?, ?, ?, ?, ?)",
        payload,
    )
    conn.execute("DELETE FROM segments_fts")
    conn.execute("INSERT INTO segments_fts(vid, text, idx) SELECT vid, text, idx FROM segments")


def load_videos(conn: sqlite3.Connection) -> list[Video]:
    rows = conn.execute("SELECT * FROM videos").fetchall()
    return [video_from_row(row) for row in rows]


def get_video(conn: sqlite3.Connection, vid: str) -> Video | None:
    row = conn.execute("SELECT * FROM videos WHERE vid = ?", (vid,)).fetchone()
    if row is None:
        return None
    return video_from_row(row)


def source_counts(conn: sqlite3.Connection) -> dict[str, int]:
    rows = conn.execute("SELECT source, COUNT(*) AS n FROM videos GROUP BY source").fetchall()
    return {str(row["source"]): int(row["n"]) for row in rows}


def video_count(conn: sqlite3.Connection) -> int:
    row = conn.execute("SELECT COUNT(*) AS n FROM videos").fetchone()
    if row is None:
        return 0
    return int(row["n"])


def related_to(conn: sqlite3.Connection, vid: str, limit: int) -> list[tuple[str, int]]:
    rows = conn.execute(
        "SELECT other_vid, shared FROM related WHERE vid = ? "
        "ORDER BY shared DESC, other_vid ASC LIMIT ?",
        (vid, limit),
    ).fetchall()
    return [(str(row["other_vid"]), int(row["shared"])) for row in rows]


def get_segment(conn: sqlite3.Connection, vid: str, idx: int) -> sqlite3.Row | None:
    return conn.execute(
        "SELECT start_s, text, url FROM segments WHERE vid = ? AND idx = ?",
        (vid, idx),
    ).fetchone()


def segment_count(conn: sqlite3.Connection) -> int:
    row = conn.execute("SELECT COUNT(*) AS n FROM segments").fetchone()
    if row is None:
        return 0
    return int(row["n"])
