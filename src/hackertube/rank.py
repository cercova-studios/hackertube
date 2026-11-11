"""Deterministic ranking. Higher affinity wins, then stored score, then vid."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import date

from hackertube.areas import affinity, matching_areas, reason_for
from hackertube.models import Video
from hackertube.state import Mark, blocks


@dataclass(frozen=True)
class Pick:
    video: Video
    area_id: str
    affinity_score: int
    reason: str


@dataclass(frozen=True)
class RankStats:
    matched: int
    hidden_news: int
    excluded_seen: int
    excluded_disliked: int
    excluded_snoozed: int

    @property
    def eligible(self) -> int:
        return self.matched


def _hidden_news(video: Video, include_news: bool) -> bool:
    return video.shelf_life == "news" and not include_news


def _source_ok(video: Video, source: str | None) -> bool:
    return source is None or video.source == source


def recommend(
    videos: list[Video],
    marks: dict[str, Mark],
    area_id: str,
    day: date,
    *,
    include_news: bool = False,
    source: str | None = None,
    limit: int = 5,
) -> tuple[list[Pick], RankStats]:
    hidden_news = 0
    excluded = {"seen": 0, "disliked": 0, "snoozed": 0}
    ranked: list[Pick] = []
    for video in videos:
        if not _source_ok(video, source):
            continue
        if video.source == "hackertube" and video.hn_recommenders <= 0:
            continue
        strength = affinity(video, area_id)
        if strength < 2:
            continue
        if _hidden_news(video, include_news):
            hidden_news += 1
            continue
        mark = marks.get(video.vid)
        if mark is not None and blocks(mark, day):
            if mark.status in excluded:
                excluded[mark.status] += 1
            continue
        ranked.append(
            Pick(
                video=video,
                area_id=area_id,
                affinity_score=strength,
                reason=reason_for(video, area_id),
            )
        )
    ranked.sort(key=lambda pick: (-pick.affinity_score, -pick.video.score, pick.video.vid))
    stats = RankStats(
        matched=len(ranked),
        hidden_news=hidden_news,
        excluded_seen=excluded["seen"],
        excluded_disliked=excluded["disliked"],
        excluded_snoozed=excluded["snoozed"],
    )
    return ranked[:limit], stats


def area_counts(
    videos: list[Video],
    marks: dict[str, Mark],
    day: date,
) -> dict[str, int]:
    counts: dict[str, int] = {}
    for area_id in ("agentic-engineering", "ai-engineering", "ai-research"):
        picks, stats = recommend(videos, marks, area_id, day, limit=10**9)
        counts[area_id] = stats.matched
        del picks
    return counts


def areas_of(video: Video) -> list[str]:
    return matching_areas(video)
