"""Command handlers. Each returns a document and an exit code. No prompts."""

from __future__ import annotations

import re
import sqlite3
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import date, timedelta
from pathlib import Path

import numpy as np

from hackertube import __version__
from hackertube.areas import AREA_IDS, AREAS, affinity, matching_areas
from hackertube.db import (
    catalog_exists,
    catalog_path,
    embed_map,
    get_segment,
    get_video,
    load_videos,
    rebuild_fts,
    related_to,
    replace_segments,
    session,
    set_transcript_path,
    source_counts,
)
from hackertube.embed import embed_status, load_matrix
from hackertube.errors import AppError, UsageError
from hackertube.insights import (
    extract,
    format_at,
    known_timestamps,
    llm_prompt,
    merge_llm_claims,
    parse_markdown,
    post_llm,
)
from hackertube.output import Table, apply_fields, shorten_scalars
from hackertube.paths import bin_path, display_path, today
from hackertube.paths import home as data_home
from hackertube.rank import area_counts, recommend
from hackertube.search import keyword_segments, search_catalog
from hackertube.session import install as install_hook
from hackertube.sources import SOURCES, IngestContext
from hackertube.state import counts, forget, load, path_for, set_mark
from hackertube.transcript import (
    fetch_captions,
    markdown_from_captions,
    read_cached,
    write_cached,
)
from hackertube.youtube_api import cache_path, duration_label, enrich, load_cache, published_at

VID_RE = re.compile(r"^[A-Za-z0-9_-]{11}$")
VIDEO_FIELDS = {
    "vid",
    "title",
    "channel",
    "score",
    "reason",
    "area",
    "source",
    "url",
    "match",
    "sim",
    "shared",
    "level",
    "topic",
    "page",
    "state",
}
INSIGHT_FIELDS = {"at", "text", "url", "title"}
MENTION_FIELDS = {"kind", "name", "at", "url"}
STATE_FIELDS = {"vid", "status", "at", "until"}


@dataclass
class Flags:
    as_json: bool
    full: bool
    fields: tuple[str, ...] | None


def run(command: str | None, args: object, flags: Flags) -> tuple[dict[str, object], int]:
    home = data_home()
    if command is None:
        return _home(flags, home)
    handler = {
        "ingest": _ingest,
        "search": _search,
        "recommend": _recommend,
        "similar": _similar,
        "show": _show,
        "transcript": _transcript,
        "insights": _insights,
        "seen": _mark,
        "like": _mark,
        "dislike": _mark,
        "snooze": _snooze,
        "forget": _forget,
        "state": _state,
        "sources": _sources,
        "areas": _areas,
        "setup": _setup,
        "enrich": _enrich,
    }.get(command)
    if handler is None:
        raise UsageError(f"unknown command {command}")
    return handler(args, flags, home)


def _finish(document: dict[str, object], flags: Flags, help_lines: list[str]) -> dict[str, object]:
    document["help"] = help_lines
    return shorten_scalars(document, full=flags.full)


def _home(flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    tallies = counts(load(home), today())
    document: dict[str, object] = {
        "description": (
            "Ranked videos for an AI/ML engineer on agentic engineering, "
            "AI engineering, and AI research."
        ),
        "version": __version__,
        "bin": bin_path(),
        "home": display_path(home),
        "catalog": display_path(catalog_path(home)),
        "state_file": display_path(path_for(home)),
        "embedder": embed_status(),
        "videos": 0,
        "state": _state_line(tallies),
    }
    help_lines = [
        "hackertube ingest --path <zip-or-dir>",
        "hackertube recommend --area agentic-engineering",
        'hackertube search "coding agents"',
    ]
    if not catalog_exists(home):
        document["empty"] = "no catalog yet"
        return _finish(document, flags, help_lines), 0
    with session(home) as conn:
        videos = load_videos(conn)
        sources = source_counts(conn)
    document["videos"] = len(videos)
    document["sources"] = _source_line(sources)
    document["state"] = _state_line(tallies)
    coverage = area_counts(videos, load(home), today())
    document["coverage"] = ", ".join(f"{area} {coverage[area]}" for area in AREA_IDS)
    marks = load(home)
    picks: list[dict[str, object]] = []
    used: set[str] = set()
    for area_id in AREA_IDS:
        ranked, _stats = recommend(videos, marks, area_id, today(), limit=5)
        for pick in ranked:
            if pick.video.vid in used:
                continue
            used.add(pick.video.vid)
            picks.append(
                {
                    "vid": pick.video.vid,
                    "title": pick.video.title,
                    "area": area_id,
                    "score": pick.video.score,
                }
            )
            break
    document["picks"] = _table(
        ("vid", "title", "area", "score"),
        picks,
        flags,
        VIDEO_FIELDS,
    )
    if not picks:
        document["empty"] = "no eligible picks"
    else:
        help_lines = [
            f"hackertube show {picks[0]['vid']}",
            "hackertube recommend --area agentic-engineering",
            f"hackertube insights {picks[0]['vid']}",
        ]
    return _finish(document, flags, help_lines), 0


def _ingest(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    selected = str(_arg(args, "source") or "all")
    names = list(SOURCES) if selected == "all" else [selected]
    limit = _arg(args, "limit")
    path_value = _arg(args, "path")
    path = Path(str(path_value)).expanduser() if path_value else None
    refresh = bool(_arg(args, "refresh"))
    rows: list[dict[str, object]] = []
    errors: list[str] = []
    with session(home, create=True) as conn:
        for name in names:
            source_path = (
                path if selected == name or (selected == "all" and name == "hackertube") else None
            )
            context = IngestContext(
                conn=conn,
                home=home,
                path=source_path,
                limit=_optional_int(limit),
                refresh=refresh,
            )
            try:
                result = SOURCES[name](context)
            except AppError as exc:
                errors.append(f"{name}: {exc.detail}")
                continue
            rows.append(
                {
                    "id": result.source,
                    "videos": result.videos,
                    "embedded": result.embedded,
                    "changed": result.changed,
                    "detail": result.detail,
                }
            )
        rebuild_fts(conn)
    document: dict[str, object] = {
        "sources": _table(
            ("id", "videos", "embedded", "changed"),
            rows,
            flags,
            {"id", "videos", "embedded", "changed", "detail"},
        ),
    }
    if rows:
        document["detail"] = " | ".join(f"{row['id']}: {row['detail']}" for row in rows)
    if errors:
        document["ok"] = False
        document["error"] = "ingest_failed"
        document["detail"] = "; ".join(errors)
        document["exit"] = 1
        return _finish(document, flags, ["hackertube ingest --help", "hackertube"]), 1
    document["ok"] = True
    return _finish(
        document, flags, ["hackertube", "hackertube recommend --area agentic-engineering"]
    ), 0


def _recommend(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    videos, marks = _catalog_videos(home)
    area = _arg(args, "area")
    limit = _bounded_limit(_arg(args, "limit"), default=5, full=flags.full)
    include_news = bool(_arg(args, "include_news"))
    source = _arg(args, "source")
    source_name = str(source) if source else None
    areas = [str(area)] if area else list(AREA_IDS)
    document: dict[str, object] = {}
    summaries: list[str] = []
    first_vid = ""
    for area_id in areas:
        ranked, stats = recommend(
            videos,
            marks,
            area_id,
            today(),
            include_news=include_news,
            source=source_name,
            limit=limit,
        )
        rows = [
            {
                "vid": pick.video.vid,
                "title": pick.video.title,
                "score": pick.video.score,
                "reason": pick.reason,
                "channel": pick.video.channel,
                "source": pick.video.source,
                "url": pick.video.url,
                "area": area_id,
                "level": pick.video.level,
                "topic": pick.video.primary_topic,
            }
            for pick in ranked
        ]
        key = "videos" if area else area_id
        document[key] = _table(("vid", "title", "score", "reason"), rows, flags, VIDEO_FIELDS)
        summaries.append(f"{area_id} {len(rows)} of {stats.matched}")
        if area:
            document["area"] = area_id
            document["count"] = f"{len(rows)} of {stats.matched} eligible"
            document["excluded"] = (
                f"seen {stats.excluded_seen}, disliked {stats.excluded_disliked}, "
                f"snoozed {stats.excluded_snoozed}"
            )
            document["hidden_news"] = stats.hidden_news
        if rows and not first_vid:
            first_vid = str(rows[0]["vid"])
        if not rows:
            document["empty"] = f"no eligible videos in {area_id}"
    if not area:
        document["count"] = ", ".join(summaries)
    help_lines = ["hackertube recommend --area ai-engineering"]
    if first_vid:
        help_lines = [
            f"hackertube show {first_vid}",
            f"hackertube insights {first_vid}",
            f"hackertube seen {first_vid}",
        ]
    elif not catalog_exists(home):
        document["empty"] = "no catalog yet"
        help_lines = ["hackertube ingest --path <zip-or-dir>"]
    return _finish(document, flags, help_lines), 0


def _search(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    query = str(_arg(args, "query") or "").strip()
    if not query:
        raise UsageError("search needs a query")
    where = str(_arg(args, "in_") or "catalog")
    limit = _bounded_limit(_arg(args, "limit"), default=8, full=flags.full)
    if not catalog_exists(home):
        document = {
            "query": query,
            "in": where,
            "count": "0 of 0",
            "empty": "no catalog yet",
            "videos": _table(("vid", "title", "score", "match"), [], flags, VIDEO_FIELDS),
        }
        return _finish(document, flags, ["hackertube ingest --path <zip-or-dir>"]), 0
    with session(home) as conn:
        if where == "transcripts":
            return _search_transcripts(conn, query, limit, flags, home)
        mapping = embed_map(conn)
        hits, mode = search_catalog(conn, query, mapping, home)
        videos = {video.vid: video for video in load_videos(conn)}
    area = _arg(args, "area")
    source = _arg(args, "source")
    rows: list[dict[str, object]] = []
    for hit in hits:
        video = videos.get(hit.vid)
        if video is None:
            continue
        if source and video.source != source:
            continue
        if area and affinity(video, str(area)) < 2:
            continue
        rows.append(
            {
                "vid": video.vid,
                "title": video.title,
                "score": video.score,
                "match": round(hit.score, 3),
                "channel": video.channel,
                "source": video.source,
                "url": video.url,
                "topic": video.primary_topic,
            }
        )
    shown = rows[:limit]
    document = {
        "query": query,
        "in": "catalog",
        "mode": mode,
        "semantic": "on" if mode == "semantic+keyword" else "off",
        "count": f"{len(shown)} of {len(rows)}",
        "videos": _table(("vid", "title", "score", "match"), shown, flags, VIDEO_FIELDS),
    }
    if not shown:
        document["empty"] = f"no videos match {query!r}"
    help_lines = ["hackertube search <query> --in transcripts"]
    if shown:
        help_lines = [f"hackertube show {shown[0]['vid']}", f"hackertube similar {shown[0]['vid']}"]
    return _finish(document, flags, help_lines), 0


def _search_transcripts(
    conn: sqlite3.Connection,
    query: str,
    limit: int,
    flags: Flags,
    home: Path,
) -> tuple[dict[str, object], int]:
    del home
    found = keyword_segments(conn, query, max(limit * 4, 20))
    rows: list[dict[str, object]] = []
    for vid, idx, score in found:
        segment = get_segment(conn, vid, idx)
        if segment is None:
            continue
        rows.append(
            {
                "vid": vid,
                "at": format_at(int(segment["start_s"])),
                "text": str(segment["text"]),
                "score": round(score, 3),
                "url": str(segment["url"]),
            }
        )
        if len(rows) >= limit:
            break
    total = len(found)
    document: dict[str, object] = {
        "query": query,
        "in": "transcripts",
        "mode": "keyword",
        "count": f"{len(rows)} of {total}",
        "segments": _table(
            ("vid", "at", "text", "score"), rows, flags, {"vid", "at", "text", "score", "url"}
        ),
    }
    if not rows:
        document["empty"] = f"no transcript segments match {query!r}"
    help_lines = ["hackertube transcript <vid>", "hackertube ingest --source aietalks"]
    if rows:
        help_lines = [
            f"hackertube insights {rows[0]['vid']}",
            f"hackertube transcript {rows[0]['vid']}",
        ]
    return _finish(document, flags, help_lines), 0


def _similar(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    vid = _require_vid(args)
    limit = _bounded_limit(_arg(args, "limit"), default=5, full=flags.full)
    with session(home) as conn:
        video = get_video(conn, vid)
        if video is None:
            raise AppError("not_found", f"no video {vid}", vid=vid)
        mapping = embed_map(conn)
        matrix = load_matrix(home)
        neighbors = _neighbors(vid, mapping, matrix, limit)
        pool = max(0, len(mapping) - (1 if vid in mapping else 0))
        if neighbors is None:
            related = related_to(conn, vid, limit)
            videos = {item.vid: item for item in load_videos(conn)}
            rows = []
            for other, shared in related:
                match = videos.get(other)
                if match is None:
                    continue
                rows.append(
                    {
                        "vid": other,
                        "title": match.title,
                        "score": match.score,
                        "shared": shared,
                    }
                )
            document: dict[str, object] = {
                "vid": vid,
                "title": video.title,
                "mode": "co-recommended",
                "count": f"{len(rows)} of {len(related)}",
                "videos": _table(("vid", "title", "score", "shared"), rows, flags, VIDEO_FIELDS),
            }
        else:
            videos = {item.vid: item for item in load_videos(conn)}
            rows = []
            for other, sim in neighbors:
                match = videos.get(other)
                if match is None:
                    continue
                rows.append(
                    {
                        "vid": other,
                        "title": match.title,
                        "score": match.score,
                        "sim": round(sim, 3),
                    }
                )
            document = {
                "vid": vid,
                "title": video.title,
                "mode": "embedding",
                "count": f"{len(rows)} of {pool}",
                "videos": _table(("vid", "title", "score", "sim"), rows, flags, VIDEO_FIELDS),
            }
    if not rows:
        document["empty"] = f"no neighbors for {vid}"
    return _finish(
        document, flags, [f"hackertube show {vid}", "hackertube recommend --area ai-engineering"]
    ), 0


def _show(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    vid = _require_vid(args)
    with session(home) as conn:
        video = get_video(conn, vid)
    if video is None:
        raise AppError("not_found", f"no video {vid}", vid=vid)
    mark = load(home).get(vid)
    areas = matching_areas(video)
    document: dict[str, object] = {
        "vid": video.vid,
        "title": video.title,
        "channel": video.channel,
        "source": video.source,
        "score": video.score,
        "areas": ",".join(areas) if areas else "none",
        "url": video.url,
        "page": video.page_url,
        "state": mark.status if mark else "unseen",
        "level": video.level,
        "content_type": video.content_type,
        "shelf_life": video.shelf_life,
        "topic": video.primary_topic,
        "summary": video.summary,
        "why": video.why,
        "published": video.published,
        "event": video.event,
        "transcript": "cached" if (home / "transcripts" / f"{vid}.md").exists() else "absent",
    }
    if video.quote:
        document["quote"] = video.quote
    cached = load_cache(home).get(vid)
    if cached:
        document["duration"] = duration_label(cached)
        document["youtube_published"] = published_at(cached)
    requested = flags.fields
    if requested:
        unknown = [field for field in requested if field not in document]
        if unknown:
            raise UsageError(f"unknown field {unknown[0]}", flag="--fields")
        document = {field: document[field] for field in requested}
    return _finish(
        document,
        flags,
        [f"hackertube insights {vid}", f"hackertube similar {vid}", f"hackertube seen {vid}"],
    ), 0


def _transcript(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    vid = _require_vid(args)
    markdown, origin = _ensure_transcript(home, vid)
    shown = markdown if flags.full else markdown[:500]
    document: dict[str, object] = {
        "vid": vid,
        "origin": origin,
        "source_url": _source_url(vid, markdown),
        "chars": len(markdown),
        "showing": f"{len(shown)} of {len(markdown)}",
        "text": shown if flags.full else _marked(markdown, 500),
    }
    return _finish(
        document, flags, [f"hackertube insights {vid}", f"hackertube transcript {vid} --full"]
    ), 0


def _insights(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    vid = _require_vid(args)
    markdown, origin = _ensure_transcript(home, vid)
    segments = parse_markdown(markdown, vid)
    scale = 8 if flags.full else 1
    insights = extract(segments, plain=markdown, vid=vid, limit_scale=scale)
    if bool(_arg(args, "llm")):
        raw = post_llm(llm_prompt(vid, markdown))
        insights = merge_llm_claims(insights, raw, known_timestamps(segments), markdown)
    document: dict[str, object] = {
        "vid": vid,
        "origin": origin,
        "method": insights.method,
        "source_url": _source_url(vid, markdown),
        "chapters_count": f"{len(insights.chapters)} of {insights.chapters_total}",
        "claims_count": f"{len(insights.claims)} of {insights.claims_total}",
        "mentions_count": f"{len(insights.mentions)} of {insights.mentions_total}",
        "quotes_count": f"{len(insights.quotes)} of {insights.quotes_total}",
        "chapters": _table(
            ("at", "text", "url"),
            [{"at": item.at, "text": item.text, "url": item.url} for item in insights.chapters],
            flags,
            INSIGHT_FIELDS,
            honor_fields=False,
        ),
        "claims": _table(
            ("at", "text", "url"),
            [{"at": item.at, "text": item.text, "url": item.url} for item in insights.claims],
            flags,
            INSIGHT_FIELDS,
            honor_fields=False,
        ),
        "mentions": _table(
            ("kind", "name", "at", "url"),
            [
                {"kind": item.kind, "name": item.name, "at": item.at, "url": item.url}
                for item in insights.mentions
            ],
            flags,
            MENTION_FIELDS,
            honor_fields=False,
        ),
        "quotes": _table(
            ("at", "text", "url"),
            [{"at": item.at, "text": item.text, "url": item.url} for item in insights.quotes],
            flags,
            INSIGHT_FIELDS,
            honor_fields=False,
        ),
    }
    if not insights.claims and not insights.quotes:
        document["empty"] = "no claims or quotes parsed"
    return _finish(document, flags, [f"hackertube transcript {vid}", f"hackertube show {vid}"]), 0


def _mark(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    verb = str(_arg(args, "command"))
    status = {"seen": "seen", "like": "liked", "dislike": "disliked"}[verb]
    return _write_mark(args, flags, home, status, until="")


def _snooze(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    until_arg = _arg(args, "until")
    days = _arg(args, "days")
    if until_arg and days is not None:
        raise UsageError("pass --until or --days, not both")
    if until_arg:
        until = str(until_arg)
        _parse_day(until)
    else:
        span = 7 if days is None else _optional_int(days) or 0
        if span < 0:
            raise UsageError("--days must be >= 0")
        until = (today() + timedelta(days=span)).isoformat()
    return _write_mark(args, flags, home, "snoozed", until=until)


def _forget(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    vid = _require_vid(args)
    changed = forget(home, vid)
    document = {
        "ok": True,
        "action": "forget",
        "vid": vid,
        "changed": changed,
        "status": "absent",
    }
    return _finish(
        document, flags, [f"hackertube show {vid}", "hackertube recommend --area ai-engineering"]
    ), 0


def _state(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    limit = _bounded_limit(_arg(args, "limit"), default=20, full=flags.full)
    marks = load(home)
    rows = [
        {"vid": vid, "status": mark.status, "at": mark.at, "until": mark.until or None}
        for vid, mark in sorted(marks.items())
    ]
    shown = rows[:limit]
    active = counts(marks, today())
    document: dict[str, object] = {
        "state_file": display_path(path_for(home)),
        "count": f"{len(shown)} of {len(rows)}",
        "active": _state_line(active),
        "items": _table(("vid", "status", "at", "until"), shown, flags, STATE_FIELDS),
    }
    if not rows:
        document["empty"] = "no saved marks"
    return _finish(
        document, flags, ["hackertube seen <vid>", "hackertube snooze <vid> --days 7"]
    ), 0


def _sources(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    del args
    loaded = {}
    if catalog_exists(home):
        with session(home) as conn:
            loaded = source_counts(conn)
    rows = [
        {
            "id": name,
            "videos": loaded.get(name, 0),
            "status": "loaded" if loaded.get(name, 0) else "empty",
        }
        for name in SOURCES
    ]
    document: dict[str, object] = {
        "sources": _table(("id", "videos", "status"), rows, flags, {"id", "videos", "status"}),
    }
    return _finish(
        document,
        flags,
        [
            "hackertube ingest --source hackertube --path <dir>",
            "hackertube ingest --source aietalks",
        ],
    ), 0


def _areas(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    del args
    videos, marks = _catalog_videos(home)
    coverage = area_counts(videos, marks, today()) if videos else {area: 0 for area in AREA_IDS}
    rows = [
        {"id": area_id, "videos": coverage[area_id], "description": AREAS[area_id].description}
        for area_id in AREA_IDS
    ]
    document: dict[str, object] = {
        "areas": _table(
            ("id", "videos", "description"), rows, flags, {"id", "videos", "description"}
        ),
    }
    return _finish(document, flags, ["hackertube recommend --area agentic-engineering"]), 0


def _setup(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    del args
    changed = install_hook(home)
    document: dict[str, object] = {
        "ok": True,
        "changed": any(changed.values()),
        "script": display_path(home / "hooks" / "session.sh"),
        "fragment": display_path(home / "hooks" / "claude.settings.fragment.json"),
        "claude_settings": display_path(Path.home() / ".claude" / "settings.json"),
        "script_changed": changed["script"],
        "claude_changed": changed["claude_settings"],
    }
    return _finish(document, flags, ["hackertube", "restart the agent session so the hook runs"]), 0


def _enrich(args: object, flags: Flags, home: Path) -> tuple[dict[str, object], int]:
    vid = _arg(args, "vid")
    limit = _bounded_limit(_arg(args, "limit"), default=50, full=flags.full)
    if vid:
        target = _require_vid(args)
        vids = [target]
    else:
        if not catalog_exists(home):
            raise AppError("catalog_missing", "ingest a catalog before enrich")
        cached = load_cache(home)
        with session(home) as conn:
            vids = [video.vid for video in load_videos(conn) if video.vid not in cached][:limit]
    fetched, already = enrich(home, vids)
    document: dict[str, object] = {
        "ok": True,
        "fetched": fetched,
        "cached": already,
        "cache": display_path(cache_path(home)),
    }
    if not vids:
        document["empty"] = "nothing to enrich"
    return _finish(document, flags, ["hackertube show <vid>"]), 0


def _write_mark(
    args: object,
    flags: Flags,
    home: Path,
    status: str,
    *,
    until: str,
) -> tuple[dict[str, object], int]:
    vid = _require_vid(args)
    _require_known(home, vid)
    changed = set_mark(home, vid, status, until=until)
    stored = load(home)[vid]
    document: dict[str, object] = {
        "ok": True,
        "action": status,
        "vid": vid,
        "changed": changed,
        "status": status,
        "at": stored.at,
    }
    if until:
        document["until"] = until
    return _finish(document, flags, ["hackertube state", f"hackertube forget {vid}"]), 0


def _ensure_transcript(home: Path, vid: str) -> tuple[str, str]:
    cached = read_cached(home, vid)
    if cached:
        origin = (
            "aietalks" if "Origin: youtube-captions" not in cached.splitlines()[:6] else "captions"
        )
        if cached.startswith("#") or "Source:" in cached[:500]:
            return cached, origin if origin == "captions" else _origin_for(home, vid, cached)
    try:
        cues = fetch_captions(vid)
    except AppError as exc:
        raise AppError(
            "transcript_unavailable",
            exc.detail,
            vid=vid,
            tried="cache,youtube_captions",
        ) from exc
    markdown = markdown_from_captions(vid, cues)
    relative = write_cached(home, vid, markdown)
    _index_transcript(home, vid, markdown, relative)
    return markdown, "captions"


def _origin_for(home: Path, vid: str, markdown: str) -> str:
    if not catalog_exists(home):
        return "cache"
    with session(home) as conn:
        video = get_video(conn, vid)
    if video is not None and video.source == "aietalks":
        return "aietalks"
    if "Origin: youtube-captions" in markdown:
        return "captions"
    return "cache"


def _index_transcript(home: Path, vid: str, markdown: str, relative: str) -> None:
    if not catalog_exists(home):
        return
    segments = [
        (idx, segment.start_s, segment.text, segment.url)
        for idx, segment in enumerate(parse_markdown(markdown, vid))
        if segment.text
    ]
    with session(home) as conn:
        if get_video(conn, vid) is None:
            return
        replace_segments(conn, vid, segments)
        set_transcript_path(conn, vid, relative)
        rebuild_fts(conn)


def _neighbors(
    vid: str,
    mapping: dict[str, int],
    matrix: np.ndarray,
    limit: int,
) -> list[tuple[str, float]] | None:
    row = mapping.get(vid)
    array = np.asarray(matrix)
    if row is None or array.shape[0] == 0 or row >= array.shape[0]:
        return None
    scores = array @ array[row]
    pairs = [
        (other, float(scores[index]))
        for other, index in mapping.items()
        if other != vid and index < scores.shape[0]
    ]
    pairs.sort(key=lambda item: (-item[1], item[0]))
    return pairs[:limit]


def _catalog_videos(home: Path) -> tuple[list, dict]:
    if not catalog_exists(home):
        return [], load(home)
    with session(home) as conn:
        return load_videos(conn), load(home)


def _require_known(home: Path, vid: str) -> None:
    if not catalog_exists(home):
        raise AppError("catalog_missing", "ingest a catalog before marking state", vid=vid)
    with session(home) as conn:
        if get_video(conn, vid) is None:
            raise AppError("not_found", f"no video {vid}", vid=vid)


def _require_vid(args: object) -> str:
    vid = str(_arg(args, "vid") or "")
    if VID_RE.fullmatch(vid) is None:
        raise UsageError("vid must be an 11-character YouTube id")
    return vid


def _table(
    default: tuple[str, ...],
    rows: Sequence[Mapping[str, object]],
    flags: Flags,
    allowed: set[str],
    *,
    honor_fields: bool = True,
) -> Table:
    requested = flags.fields if honor_fields else None
    return apply_fields(rows, default, requested, allowed)


def _optional_int(value: object) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int | str):
        raise UsageError("expected an integer")
    return int(value)


def _bounded_limit(value: object, *, default: int, full: bool) -> int:
    parsed_value = _optional_int(value)
    if parsed_value is None:
        return default
    parsed = parsed_value
    if parsed < 1:
        raise UsageError("--limit must be >= 1")
    cap = 200 if full else 50
    if parsed > cap:
        raise UsageError(f"--limit max is {cap}")
    return parsed


def _state_line(tallies: dict[str, int]) -> str:
    return (
        f"seen {tallies.get('seen', 0)}, liked {tallies.get('liked', 0)}, "
        f"disliked {tallies.get('disliked', 0)}, snoozed {tallies.get('snoozed', 0)}"
    )


def _source_line(sources: dict[str, int]) -> str:
    if not sources:
        return "none"
    return ", ".join(f"{name} {sources[name]}" for name in sorted(sources))


def _source_url(vid: str, markdown: str) -> str:
    for line in markdown.splitlines():
        if line.startswith("Source: "):
            return line.removeprefix("Source: ").strip()
        if "youtube.com/watch" in line:
            match = re.search(r"https://www\.youtube\.com/watch\?v=[A-Za-z0-9_-]{11}", line)
            if match:
                return match.group(0)
    return f"https://www.youtube.com/watch?v={vid}"


def _marked(text: str, limit: int) -> str:
    if len(text) <= limit:
        return text
    cut = text[:limit].rstrip()
    return f"{cut}…[+{len(text) - len(cut)}c]"


def _parse_day(value: str) -> None:
    try:
        date.fromisoformat(value)
    except ValueError as exc:
        raise UsageError("--until must be YYYY-MM-DD") from exc


def _arg(args: object, name: str) -> object:
    return getattr(args, name)
