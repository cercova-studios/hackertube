"""Resolve a transcript: cached AI Engineer markdown, else YouTube captions."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from hackertube.errors import AppError
from hackertube.insights import Segment, parse_markdown, segments_from_captions, youtube_url

try:
    from youtube_transcript_api import YouTubeTranscriptApi
except ImportError:  # optional extra: uv sync --extra captions
    YouTubeTranscriptApi = None

_SOURCE_LINE = re.compile(
    r"^Source:\s*(https://www\.youtube\.com/watch\?v=([A-Za-z0-9_-]{11}))\s*$",
    re.MULTILINE,
)
_PUBLISHED_LINE = re.compile(r"^Published:\s*(\d{4}-\d{2}-\d{2})\s*$", re.MULTILINE)
_WATCH = re.compile(r"https://www\.youtube\.com/watch\?v=([A-Za-z0-9_-]{11})")


def parse_identity(markdown: str) -> tuple[str, str]:
    """Return (video id, published date). Date is empty when the page has none.

    AIE Talks pages carry ``Source:`` and ``Published:`` lines. Official
    ai.engineer markdown carries a YouTube link instead of those lines.
    """
    source = _SOURCE_LINE.search(markdown)
    published = _PUBLISHED_LINE.search(markdown)
    published_on = published.group(1) if published else ""
    if source:
        return source.group(2), published_on
    watch = _WATCH.search(markdown)
    if watch:
        return watch.group(1), published_on
    raise AppError("transcript_identity", "markdown has no Source line or YouTube video id")


def read_cached(home: Path, vid: str) -> str | None:
    path = home / "transcripts" / f"{vid}.md"
    if not path.exists():
        return None
    return path.read_text(encoding="utf-8")


def write_cached(home: Path, vid: str, markdown: str) -> str:
    directory = home / "transcripts"
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{vid}.md"
    path.write_text(markdown, encoding="utf-8")
    return f"transcripts/{vid}.md"


def markdown_from_captions(vid: str, cues: list[tuple[float, str]]) -> str:
    lines = [
        f"Source: {youtube_url(vid)}",
        "Origin: youtube-captions",
        "",
        "## Full transcript",
        "",
    ]
    for start, text in cues:
        seconds = int(start)
        cleaned = " ".join(text.split())
        if not cleaned:
            continue
        lines.append(f"[{_clock(seconds)}]({youtube_url(vid, seconds)}) {cleaned}")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def fetch_captions(vid: str) -> list[tuple[float, str]]:
    if YouTubeTranscriptApi is None:
        raise AppError(
            "captions_unavailable",
            "youtube-transcript-api is not installed (uv sync --extra captions)",
        )
    api_cls: Any = YouTubeTranscriptApi
    try:
        api = api_cls()
        fetched = api.fetch(vid)
        cues = [(float(snippet.start), str(snippet.text)) for snippet in fetched]
    except AttributeError:
        cues = _legacy_captions(api_cls, vid)
    except Exception as exc:  # noqa: BLE001 — provider errors become a structured result
        raise AppError("transcript_unavailable", f"captions failed: {exc}") from exc
    if not cues:
        raise AppError("transcript_unavailable", "captions response was empty")
    return cues


def _legacy_captions(api_cls: Any, vid: str) -> list[tuple[float, str]]:
    getter = getattr(api_cls, "get_transcript", None)
    if getter is None:
        raise AppError("transcript_unavailable", "youtube-transcript-api has no fetch method")
    try:
        raw = getter(vid)
    except Exception as exc:  # noqa: BLE001
        raise AppError("transcript_unavailable", f"captions failed: {exc}") from exc
    cues: list[tuple[float, str]] = []
    for item in raw:
        cues.append((float(item["start"]), str(item["text"])))
    return cues


def load_segments(markdown: str, vid: str) -> list[Segment]:
    return parse_markdown(markdown, vid)


def caption_segments(vid: str, cues: list[tuple[float, str]]) -> list[Segment]:
    return segments_from_captions(vid, cues)


def _clock(seconds: int) -> str:
    hours, rem = divmod(seconds, 3600)
    minutes, secs = divmod(rem, 60)
    if hours:
        return f"{hours}:{minutes:02d}:{secs:02d}"
    return f"{minutes}:{secs:02d}"
