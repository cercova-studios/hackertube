"""Catalog row used by ranking, search, and commands."""

from __future__ import annotations

import json
import sqlite3
from dataclasses import dataclass


@dataclass(frozen=True)
class Video:
    vid: str
    source: str
    url: str
    page_url: str
    title: str
    channel: str
    summary: str
    why: str
    content_type: str
    level: str
    shelf_life: str
    primary_topic: str
    tags: tuple[str, ...]
    category_ids: tuple[str, ...]
    topic_slugs: tuple[str, ...]
    score: float
    published: str
    event: str
    hn_recommenders: int
    quote: str
    embed_row: int | None
    transcript_path: str

    def blob(self) -> str:
        tags = " ".join(self.tags)
        topics = " ".join(self.topic_slugs)
        return " ".join(
            (
                self.title,
                self.channel,
                self.summary,
                self.why,
                self.primary_topic,
                tags,
                topics,
                " ".join(self.category_ids),
            )
        ).lower()


def _loads(value: str | None, default: list[str]) -> tuple[str, ...]:
    if not value:
        return tuple(default)
    parsed = json.loads(value)
    if not isinstance(parsed, list):
        return tuple(default)
    return tuple(str(item) for item in parsed)


def video_from_row(row: sqlite3.Row) -> Video:
    embed = row["embed_row"]
    return Video(
        vid=str(row["vid"]),
        source=str(row["source"]),
        url=str(row["url"]),
        page_url=str(row["page_url"] or ""),
        title=str(row["title"]),
        channel=str(row["channel"] or ""),
        summary=str(row["summary"] or ""),
        why=str(row["why"] or ""),
        content_type=str(row["content_type"] or ""),
        level=str(row["level"] or ""),
        shelf_life=str(row["shelf_life"] or ""),
        primary_topic=str(row["primary_topic"] or ""),
        tags=_loads(row["tags_json"], []),
        category_ids=_loads(row["categories_json"], []),
        topic_slugs=_loads(row["topics_json"], []),
        score=float(row["score"] or 0),
        published=str(row["published"] or ""),
        event=str(row["event"] or ""),
        hn_recommenders=int(row["hn_recommenders"] or 0),
        quote=str(row["quote"] or ""),
        embed_row=None if embed is None else int(embed),
        transcript_path=str(row["transcript_path"] or ""),
    )
