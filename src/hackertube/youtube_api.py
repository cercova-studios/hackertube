"""Optional YouTube Data API enrichment. Never called unless YOUTUBE_API_KEY is set.

Responses are cached under the home directory and are not redistributed.
YouTube's terms require the key to stay private and cached metadata to be refreshed.
"""

from __future__ import annotations

import json
import os
import re
import urllib.parse
import urllib.request
from pathlib import Path

from hackertube.errors import AppError

_API = "https://www.googleapis.com/youtube/v3/videos"


def cache_path(home: Path) -> Path:
    return home / "cache" / "youtube_meta.jsonl"


def load_cache(home: Path) -> dict[str, object]:
    path = cache_path(home)
    if not path.exists():
        return {}
    found: dict[str, object] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        if isinstance(row, dict) and row.get("vid"):
            found[str(row["vid"])] = row.get("item")
    return found


def enrich(home: Path, vids: list[str]) -> tuple[int, int]:
    """Fetch metadata for vids not already cached. Returns (fetched, already_cached)."""
    key = os.environ.get("YOUTUBE_API_KEY", "")
    if not key:
        raise AppError("youtube_unconfigured", "set YOUTUBE_API_KEY or skip enrich")
    cached = load_cache(home)
    todo = [vid for vid in vids if vid not in cached]
    if not todo:
        return 0, len(vids)
    path = cache_path(home)
    path.parent.mkdir(parents=True, exist_ok=True)
    fetched = 0
    with path.open("a", encoding="utf-8") as handle:
        for start in range(0, len(todo), 50):
            batch = todo[start : start + 50]
            items = _fetch_batch(batch, key)
            for vid in batch:
                handle.write(json.dumps({"vid": vid, "item": items.get(vid)}) + "\n")
                fetched += 1
    return fetched, len(vids) - len(todo)


def _fetch_batch(vids: list[str], key: str) -> dict[str, object]:
    query = urllib.parse.urlencode(
        {"part": "snippet,contentDetails,statistics,status", "id": ",".join(vids), "key": key}
    )
    request = urllib.request.Request(f"{_API}?{query}", headers={"User-Agent": "hackertube/0.1"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except Exception as exc:  # noqa: BLE001
        raise AppError("youtube_failed", str(exc)) from exc
    items = payload.get("items") if isinstance(payload, dict) else None
    if not isinstance(items, list):
        raise AppError("youtube_failed", "videos.list response had no items list")
    return {str(item["id"]): item for item in items if isinstance(item, dict) and item.get("id")}


def duration_label(item: object) -> str:
    if not isinstance(item, dict):
        return ""
    details = item.get("contentDetails")
    if not isinstance(details, dict):
        return ""
    raw = str(details.get("duration") or "")
    return _iso_duration(raw)


def published_at(item: object) -> str:
    if not isinstance(item, dict):
        return ""
    snippet = item.get("snippet")
    if not isinstance(snippet, dict):
        return ""
    return str(snippet.get("publishedAt") or "")


def _iso_duration(value: str) -> str:
    if not value.startswith("PT"):
        return value
    hours = _part(value, "H")
    minutes = _part(value, "M")
    seconds = _part(value, "S")
    if hours:
        return f"{hours}:{minutes:02d}:{seconds:02d}"
    return f"{minutes}:{seconds:02d}"


def _part(value: str, unit: str) -> int:
    match = re.search(rf"(\d+){unit}", value)
    if match is None:
        return 0
    return int(match.group(1))
