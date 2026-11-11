"""AI Engineer talks, source id ``aietalks``.

Pipeline, ported from 10x-swe ``catalog.py`` source ``aietalks`` and pointed at
the corpus this CLI ranks:

1. GET https://ai.engineer/api/data/transcripts?format=json
2. GET https://ai.engineer/api/data/talks?format=json and map ``talkSlug`` to a
   YouTube ``videoId`` (the transcript index does not carry video ids).
3. GET ``{talk.url}.md``.
4. Read ``Source:`` / ``Published:`` when the markdown has them (AIE Talks
   write-ups). Official ai.engineer pages omit those lines; the YouTube id then
   comes from the talks join, and any watch URL in the body must match.

Only https://ai.engineer and https://aietalks.com are fetched. Markdown and the
two indexes are cached under the home directory. Embeddings use the same
BAAI/bge-small-en-v1.5 model as the HackerTube matrix, with no query prefix.
"""

from __future__ import annotations

import hashlib
import json
import re
import urllib.request
from pathlib import Path
from urllib.parse import urlsplit

from hackertube.areas import AREAS
from hackertube.db import (
    embed_map,
    meta_get,
    meta_set,
    replace_segments,
    set_embed_row,
    upsert_video,
)
from hackertube.embed import embed_texts, merge_rows
from hackertube.errors import AppError
from hackertube.insights import parse_markdown
from hackertube.paths import now_iso
from hackertube.sources.base import IngestContext, SourceResult
from hackertube.transcript import parse_identity, write_cached

TRANSCRIPTS_URL = "https://ai.engineer/api/data/transcripts?format=json"
TALKS_URL = "https://ai.engineer/api/data/talks?format=json"
_ALLOWED_HOSTS = frozenset({"ai.engineer", "www.ai.engineer", "aietalks.com", "www.aietalks.com"})
_VID = re.compile(r"^[A-Za-z0-9_-]{11}$")
_DEFAULT_LIMIT = 20
_AGENTIC = AREAS["agentic-engineering"].topic_slugs
_SPECIFIC = frozenset(
    {
        "harness-engineering",
        "coding-agents",
        "model-context-protocol",
        "agent-skills",
        "multi-agent-systems",
        "agent-orchestration",
    }
)


def ingest_aietalks(ctx: IngestContext) -> SourceResult:
    limit = _DEFAULT_LIMIT if ctx.limit is None else ctx.limit
    if limit < 0:
        raise AppError("bad_limit", "--limit must be >= 0; use 0 for the full index")
    transcripts, talks = _load_indexes(ctx)
    talk_by_slug = {str(talk.get("slug")): talk for talk in talks if isinstance(talk, dict)}
    selected = _select(transcripts, talk_by_slug, limit)
    if not selected and transcripts:
        raise AppError("aietalks_empty", "transcript index did not match any talk with a video id")
    records: list[dict[str, object]] = []
    embed_inputs: list[tuple[str, str]] = []
    skipped = 0
    for transcript_row, talk in selected:
        try:
            record, markdown = _materialize(ctx, transcript_row, talk)
        except AppError:
            skipped += 1
            continue
        records.append(record)
        embed_inputs.append((str(record["vid"]), _embed_text(record)))
        segments = parse_markdown(markdown, str(record["vid"]))
        replace_segments(
            ctx.conn,
            str(record["vid"]),
            [
                (idx, segment.start_s, segment.text, segment.url)
                for idx, segment in enumerate(segments)
                if segment.text
            ],
        )
    digest = _digest(records)
    previous = meta_get(ctx.conn, "aietalks_sha256")
    embedded = _embed(ctx, embed_inputs)
    changed = previous != digest or ctx.refresh
    for record in records:
        record["embed_row"] = embed_map(ctx.conn).get(str(record["vid"]))
        record["updated_at"] = now_iso()
        upsert_video(ctx.conn, record)
    meta_set(ctx.conn, "aietalks_sha256", digest)
    total = len(transcripts)
    detail = f"ingested {len(records)} of {total} talks"
    if limit and total > len(records):
        detail += f"; index has {total}; pass --limit 0 for all"
    if skipped:
        detail += f"; skipped {skipped}"
    return SourceResult("aietalks", len(records), embedded, changed, detail)


def _select(
    transcripts: list[dict[str, object]],
    talk_by_slug: dict[str, dict[str, object]],
    limit: int,
) -> list[tuple[dict[str, object], dict[str, object]]]:
    paired: list[tuple[dict[str, object], dict[str, object]]] = []
    for row in transcripts:
        slug = str(row.get("talkSlug") or "")
        talk = talk_by_slug.get(slug)
        if talk is None:
            continue
        vid = str(talk.get("videoId") or "")
        if _VID.fullmatch(vid) is None:
            continue
        paired.append((row, talk))
        if limit and len(paired) >= limit:
            break
    return paired


def _materialize(
    ctx: IngestContext,
    transcript_row: dict[str, object],
    talk: dict[str, object],
) -> tuple[dict[str, object], str]:
    slug = str(talk.get("slug") or transcript_row.get("talkSlug") or "")
    vid = str(talk["videoId"])
    page = str(talk.get("url") or "")
    markdown = _markdown_for(ctx, vid, slug, page)
    found_vid, published = parse_identity(markdown)
    if found_vid != vid:
        raise AppError("transcript_identity", f"{slug} markdown video {found_vid} != {vid}")
    speakers = _speakers(talk.get("speakers"))
    topics = _topics(talk.get("topics"))
    summary = str(talk.get("summary") or "")
    title = str(talk.get("title") or transcript_row.get("title") or slug)
    record: dict[str, object] = {
        "vid": vid,
        "source": "aietalks",
        "url": f"https://www.youtube.com/watch?v={vid}",
        "page_url": page,
        "title": title,
        "channel": speakers or "AI Engineer",
        "summary": summary,
        "why": _why(summary, topics),
        "content_type": "talk",
        "level": "intermediate",
        "shelf_life": "evergreen",
        "primary_topic": topics[0][1] if topics else "",
        "tags_json": json.dumps([name for _slug, name in topics]),
        "categories_json": "[]",
        "topics_json": json.dumps([slug_name for slug_name, _name in topics]),
        "quote": "",
        "hn_recommenders": 0,
        "score": _score(topics),
        "published": published,
        "event": str(talk.get("event") or ""),
        "transcript_path": f"transcripts/{vid}.md",
        "embed_row": None,
        "updated_at": "",
    }
    return record, markdown


def _markdown_for(ctx: IngestContext, vid: str, slug: str, page: str) -> str:
    if ctx.path is not None:
        text = _local_markdown(ctx.path, vid, slug)
    else:
        cache = ctx.home / "cache" / "aietalks" / f"{vid}.md"
        if cache.exists() and not ctx.refresh:
            text = cache.read_text(encoding="utf-8")
        else:
            if not page:
                raise AppError("aietalks_url", f"{slug} has no talk url")
            text = _fetch_text(_markdown_url(page), cap=2_000_000)
            cache.parent.mkdir(parents=True, exist_ok=True)
            cache.write_text(text, encoding="utf-8")
    write_cached(ctx.home, vid, text)
    return text


def _markdown_url(page: str) -> str:
    if page.endswith(".md"):
        return page
    return page + ".md"


def _local_markdown(root: Path, vid: str, slug: str) -> str:
    for candidate in (
        root / "markdown" / f"{vid}.md",
        root / "markdown" / f"{slug}.md",
        root / f"{vid}.md",
        root / f"{slug}.md",
    ):
        if candidate.exists():
            return candidate.read_text(encoding="utf-8")
    raise AppError("aietalks_missing", f"no markdown for {vid} under {root}")


def _load_indexes(ctx: IngestContext) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    if ctx.path is not None:
        transcripts = _read_json(ctx.path / "transcripts.json")
        talks = _read_json(ctx.path / "talks.json")
        return transcripts, talks
    cache = ctx.home / "cache" / "aietalks"
    cache.mkdir(parents=True, exist_ok=True)
    transcripts = _cached_json(cache / "transcripts.json", TRANSCRIPTS_URL, ctx.refresh, 8_000_000)
    talks = _cached_json(cache / "talks.json", TALKS_URL, ctx.refresh, 40_000_000)
    return transcripts, talks


def _cached_json(path: Path, url: str, refresh: bool, cap: int) -> list[dict[str, object]]:
    if path.exists() and not refresh:
        return _read_json(path)
    text = _fetch_text(url, cap=cap)
    path.write_text(text, encoding="utf-8")
    return _read_json(path)


def _read_json(path: Path) -> list[dict[str, object]]:
    if not path.exists():
        raise AppError("aietalks_missing", f"missing {path.name}")
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, list):
        raise AppError("aietalks_schema", f"{path.name} must be a JSON list")
    return [row for row in payload if isinstance(row, dict)]


def _fetch_text(url: str, *, cap: int) -> str:
    parts = urlsplit(url)
    if parts.scheme != "https" or parts.hostname not in _ALLOWED_HOSTS:
        raise AppError("disallowed_url", f"refusing {url}")
    request = urllib.request.Request(
        url, headers={"User-Agent": "hackertube/0.1", "Accept": "text/markdown, application/json"}
    )
    try:
        with urllib.request.urlopen(request, timeout=40) as response:
            raw = response.read(cap + 1)
    except AppError:
        raise
    except Exception as exc:  # noqa: BLE001
        raise AppError("aietalks_fetch", f"{url} failed: {exc}") from exc
    if len(raw) > cap:
        raise AppError("aietalks_fetch", f"{url} exceeded {cap} bytes")
    return raw.decode("utf-8")


def _speakers(value: object) -> str:
    if not isinstance(value, list):
        return ""
    names = [str(item.get("name")) for item in value if isinstance(item, dict) and item.get("name")]
    return ", ".join(names)


def _topics(value: object) -> list[tuple[str, str]]:
    if not isinstance(value, list):
        return []
    topics: list[tuple[str, str]] = []
    for item in value:
        if isinstance(item, dict) and item.get("slug"):
            topics.append((str(item["slug"]), str(item.get("name") or item["slug"])))
    return topics


def _why(summary: str, topics: list[tuple[str, str]]) -> str:
    cleaned = " ".join(summary.split())
    if cleaned:
        for mark in (". ", "? "):
            if mark in cleaned:
                return cleaned.split(mark, 1)[0] + mark.strip()
        return cleaned
    if topics:
        return f"AI Engineer talk on {topics[0][1]}"
    return "AI Engineer talk"


def _score(topics: list[tuple[str, str]]) -> float:
    slugs = {slug for slug, _name in topics}
    score = 4.0
    if slugs & _AGENTIC:
        score += 1.5
    if slugs & _SPECIFIC:
        score += 1.5
    return min(score, 9.0)


def _embed_text(record: dict[str, object]) -> str:
    tags = " ".join(json.loads(str(record["tags_json"])))
    return "\n".join(
        (
            str(record["title"]),
            str(record["channel"]),
            tags,
            str(record["summary"]),
            str(record["why"]),
        )
    )


def _embed(ctx: IngestContext, pairs: list[tuple[str, str]]) -> int:
    if not pairs:
        return 0
    vectors = embed_texts([text for _vid, text in pairs])
    if vectors is None:
        return 0
    rows = [(vid, vectors[index]) for index, (vid, _text) in enumerate(pairs)]
    mapping = merge_rows(ctx.home, rows, embed_map(ctx.conn))
    for vid, _vector in rows:
        set_embed_row(ctx.conn, vid, mapping[vid])
    return len(rows)


def _digest(records: list[dict[str, object]]) -> str:
    digest = hashlib.sha256()
    for record in sorted(records, key=lambda item: str(item["vid"])):
        digest.update(str(record["vid"]).encode())
        digest.update(b"\0")
        digest.update(str(record["title"]).encode())
        digest.update(b"\0")
        digest.update(str(record["published"]).encode())
        digest.update(b"\n")
    return digest.hexdigest()
