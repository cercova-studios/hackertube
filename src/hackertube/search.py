"""Keyword search over SQLite FTS, blended with cosine similarity when embeddings exist."""

from __future__ import annotations

import re
import sqlite3
from dataclasses import dataclass
from pathlib import Path

from hackertube.embed import semantic_scores


@dataclass(frozen=True)
class Hit:
    vid: str
    score: float
    mode: str


def fts_query(text: str) -> str:
    tokens = re.findall(r"[A-Za-z0-9]{2,}", text.lower())
    if not tokens:
        return ""
    return " OR ".join(f'"{token}"' for token in tokens[:12])


def keyword_videos(conn: sqlite3.Connection, query: str, limit: int) -> dict[str, float]:
    match = fts_query(query)
    if not match:
        return {}
    try:
        rows = conn.execute(
            "SELECT vid, bm25(videos_fts) AS rank FROM videos_fts "
            "WHERE videos_fts MATCH ? ORDER BY rank LIMIT ?",
            (match, limit),
        ).fetchall()
    except sqlite3.OperationalError:
        return {}
    return _normalize_bm25([(str(row["vid"]), float(row["rank"])) for row in rows])


def keyword_segments(
    conn: sqlite3.Connection, query: str, limit: int
) -> list[tuple[str, int, float]]:
    match = fts_query(query)
    if not match:
        return []
    try:
        rows = conn.execute(
            "SELECT vid, idx, bm25(segments_fts) AS rank FROM segments_fts "
            "WHERE segments_fts MATCH ? ORDER BY rank LIMIT ?",
            (match, limit),
        ).fetchall()
    except sqlite3.OperationalError:
        return []
    weights = _normalize_bm25([(f"{row['vid']}:{row['idx']}", float(row["rank"])) for row in rows])
    ordered: list[tuple[str, int, float]] = []
    for row in rows:
        key = f"{row['vid']}:{row['idx']}"
        ordered.append((str(row["vid"]), int(row["idx"]), weights.get(key, 0.0)))
    ordered.sort(key=lambda item: (-item[2], item[0], item[1]))
    return ordered


def blend(
    keyword: dict[str, float],
    semantic: dict[str, float] | None,
) -> list[Hit]:
    if semantic is None:
        hits = [Hit(vid, score, "keyword") for vid, score in keyword.items()]
        hits.sort(key=lambda hit: (-hit.score, hit.vid))
        return hits
    top_semantic = dict(sorted(semantic.items(), key=lambda item: -item[1])[:50])
    ids = set(keyword) | set(top_semantic)
    hits = []
    for vid in ids:
        semantic_score = max(0.0, top_semantic.get(vid, 0.0))
        keyword_score = keyword.get(vid, 0.0)
        if vid not in top_semantic and vid not in keyword:
            continue
        combined = 0.7 * semantic_score + 0.3 * keyword_score
        if combined <= 0:
            continue
        hits.append(Hit(vid, combined, "semantic+keyword"))
    hits.sort(key=lambda hit: (-hit.score, hit.vid))
    return hits


def search_catalog(
    conn: sqlite3.Connection,
    query: str,
    mapping: dict[str, int],
    data_home: Path,
) -> tuple[list[Hit], str]:
    keyword = keyword_videos(conn, query, 50)
    semantic = semantic_scores(data_home, query, mapping) if mapping else None
    mode = "semantic+keyword" if semantic is not None else "keyword"
    if semantic is not None and not semantic and keyword:
        mode = "keyword"
        semantic = None
    return blend(keyword, semantic), mode


def _normalize_bm25(rows: list[tuple[str, float]]) -> dict[str, float]:
    if not rows:
        return {}
    # sqlite bm25 is lower (more negative) for better matches.
    strengths = {key: -value for key, value in rows}
    peak = max(strengths.values())
    if peak <= 0:
        return {key: 0.0 for key in strengths}
    return {key: value / peak for key, value in strengths.items()}
