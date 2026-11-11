"""Deterministic insight extraction from a transcript.

Ported in spirit from 10x-swe's distiller, which only pulls a YouTube page's
title and description HTML (see docs/adr/0005-insight-extraction.md). This
module reads timestamped markdown or caption segments and keeps a source URL
on every item. An LLM pass is optional and cannot invent timestamps.
"""

from __future__ import annotations

import json
import os
import re
import urllib.request
from dataclasses import dataclass

from hackertube.errors import AppError

_TS = re.compile(r"(?:(\d{1,2}):)?(\d{1,2}):(\d{2})")
_TS_LINK = re.compile(
    r"\[((?:\d{1,2}:)?\d{1,2}:\d{2})\]\((https://www\.youtube\.com/watch\?v=[^)\s]+)\)"
)
_ARXIV = re.compile(r"https?://arxiv\.org/(?:abs|pdf)/(\d{4}\.\d{4,5})(?:v\d+)?", re.I)
_GITHUB = re.compile(r"https?://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)")
_HEADING = re.compile(r"^(#{1,6})\s+(.+?)\s*$")
_DENY = frozenset(
    {
        "tl;dr",
        "tldr",
        "summary",
        "key ideas",
        "key takeaways",
        "slides",
        "full transcript",
        "quotes",
        "tags",
        "references",
        "transcript",
        "anthropic slide decks",
    }
)
_STAGE = re.compile(r"\[(?:music|applause|laughter|upbeat|cheering)[^\]]*\]", re.I)
_CLAIM_CUE = re.compile(
    r"\b(should|must|means|because|need to|is the|percent|%|instead)\b",
    re.I,
)
_TOOLS: tuple[tuple[str, str], ...] = (
    ("Model Context Protocol", "MCP"),
    ("GitHub Copilot", "GitHub Copilot"),
    ("Claude Code", "Claude Code"),
    ("Claude Agent SDK", "Claude Agent SDK"),
    ("SWE-bench", "SWE-bench"),
    ("LlamaIndex", "LlamaIndex"),
    ("LangChain", "LangChain"),
    ("Stagehand", "Stagehand"),
    ("Browserbase", "Browserbase"),
    ("pgvector", "pgvector"),
    ("FastAPI", "FastAPI"),
    ("PyTorch", "PyTorch"),
    ("vLLM", "vLLM"),
    ("Ollama", "Ollama"),
    ("ChatGPT", "ChatGPT"),
    ("SKILL.md", "SKILL.md"),
    ("MCP", "MCP"),
    ("RAG", "RAG"),
)


@dataclass(frozen=True)
class Segment:
    start_s: int
    url: str
    text: str
    heading: str
    section: str


@dataclass(frozen=True)
class Item:
    at: str
    text: str
    url: str


@dataclass(frozen=True)
class Mention:
    kind: str
    name: str
    at: str
    url: str


@dataclass(frozen=True)
class Insights:
    chapters: list[Item]
    claims: list[Item]
    mentions: list[Mention]
    quotes: list[Item]
    chapters_total: int
    claims_total: int
    mentions_total: int
    quotes_total: int
    method: str


def _by_time(item: Item) -> tuple[int, str]:
    return (parse_timestamp(item.at) or 0, item.text)


def format_at(seconds: int) -> str:
    hours, rem = divmod(max(0, seconds), 3600)
    minutes, secs = divmod(rem, 60)
    if hours:
        return f"{hours}:{minutes:02d}:{secs:02d}"
    return f"{minutes}:{secs:02d}"


def parse_timestamp(value: str) -> int | None:
    match = _TS.fullmatch(value.strip())
    if match is None:
        return None
    hours = int(match.group(1) or 0)
    minutes = int(match.group(2))
    seconds = int(match.group(3))
    return hours * 3600 + minutes * 60 + seconds


def youtube_url(vid: str, start_s: int | None = None) -> str:
    base = f"https://www.youtube.com/watch?v={vid}"
    if start_s is None:
        return base
    return f"{base}&t={start_s}s"


def parse_markdown(text: str, vid: str) -> list[Segment]:
    """Timestamped lines become segments. A link on its own line borrows the next paragraph."""
    lines = [raw.strip() for raw in text.splitlines()]
    h2 = ""
    h3 = ""
    segments: list[Segment] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        index += 1
        if not line:
            continue
        heading_match = _HEADING.match(line)
        if heading_match:
            title = heading_match.group(2).strip()
            if len(heading_match.group(1)) <= 2:
                h2 = title
                h3 = ""
            else:
                h3 = title
            continue
        links = list(_TS_LINK.finditer(line))
        if not links:
            continue
        body = _clean_body(_TS_LINK.sub("", line))
        if not body and index < len(lines):
            nxt = lines[index]
            if nxt and _HEADING.match(nxt) is None and _TS_LINK.search(nxt) is None:
                body = _clean_body(nxt)
                index += 1
        start = parse_timestamp(links[0].group(1))
        if start is None:
            continue
        url = links[0].group(2)
        if vid not in url:
            url = youtube_url(vid, start)
        segments.append(Segment(start, url, body, h3 or h2, h2))
    return segments


def _clean_body(line: str) -> str:
    body = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", line)
    body = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", body)
    body = _STAGE.sub("", body)
    body = re.sub(r"^[-*]\s+", "", body)
    return " ".join(body.split()).strip(" -")


def segments_from_captions(vid: str, cues: list[tuple[float, str]]) -> list[Segment]:
    rows: list[Segment] = []
    for start, text in cues:
        cleaned = _STAGE.sub("", text)
        cleaned = " ".join(cleaned.split())
        if not cleaned:
            continue
        seconds = int(start)
        rows.append(
            Segment(
                seconds, youtube_url(vid, seconds), cleaned, "Full transcript", "Full transcript"
            )
        )
    return rows


def extract(
    segments: list[Segment],
    *,
    plain: str = "",
    vid: str = "",
    limit_scale: int = 1,
) -> Insights:
    chapters = sorted(_chapters(segments), key=_by_time)
    claims = _claims(segments)
    if not claims:
        claims = _fallback_claims(segments)
    claims = sorted(claims, key=_by_time)
    mentions = _mentions(segments)
    if plain and vid:
        mentions = _mentions_in_plain(plain, vid, mentions)
    quotes = sorted(_quotes(segments), key=_by_time)
    chapter_cap = 8 * limit_scale
    claim_cap = 6 * limit_scale
    mention_cap = 8 * limit_scale
    quote_cap = 3 * limit_scale
    return Insights(
        chapters=chapters[:chapter_cap],
        claims=claims[:claim_cap],
        mentions=mentions[:mention_cap],
        quotes=quotes[:quote_cap],
        chapters_total=len(chapters),
        claims_total=len(claims),
        mentions_total=len(mentions),
        quotes_total=len(quotes),
        method="deterministic",
    )


def _denied(title: str) -> bool:
    normalized = title.strip().lower().rstrip(":")
    return normalized in _DENY


def _chapters(segments: list[Segment]) -> list[Item]:
    seen: set[str] = set()
    items: list[Item] = []
    for segment in segments:
        title = segment.heading.strip()
        if not title or _denied(title) or title in seen:
            continue
        if segment.section.lower().rstrip(":") in _DENY and title == segment.section:
            continue
        seen.add(title)
        items.append(Item(format_at(segment.start_s), title, segment.url))
    return items


def _claims(segments: list[Segment]) -> list[Item]:
    def under(name: str) -> list[Segment]:
        return [
            segment
            for segment in segments
            if segment.section.lower().rstrip(":") == name and len(segment.text.strip()) >= 40
        ]

    chosen = under("key ideas") or under("key takeaways")
    items: list[Item] = []
    seen: set[str] = set()
    for segment in chosen:
        text = segment.text.strip()
        key = text.casefold()
        if key in seen:
            continue
        seen.add(key)
        items.append(Item(format_at(segment.start_s), text, segment.url))
    return items


def _fallback_claims(segments: list[Segment]) -> list[Item]:
    items: list[Item] = []
    for segment in segments:
        if segment.section.lower().rstrip(":") not in {"full transcript", "transcript", ""}:
            if "transcript" not in segment.section.lower() and segment.heading != "Full transcript":
                continue
        text = segment.text.strip()
        if not (40 <= len(text) <= 320):
            continue
        if not _CLAIM_CUE.search(text):
            continue
        items.append(Item(format_at(segment.start_s), text, segment.url))
    return items


def _mentions(segments: list[Segment]) -> list[Mention]:
    found: list[Mention] = []
    seen: set[tuple[str, str]] = set()
    for segment in segments:
        text = segment.text
        at = format_at(segment.start_s)
        for match in _GITHUB.finditer(text):
            owner, repo = match.group(1), match.group(2)
            if owner in {"topics", "orgs", "features"} or repo in {"blob", "tree", "issues"}:
                continue
            name = f"github.com/{owner}/{repo}"
            _add_mention(found, seen, "repo", name, at, segment.url)
        for match in _ARXIV.finditer(text):
            name = f"arxiv:{match.group(1)}"
            _add_mention(found, seen, "paper", name, at, segment.url)
        occupied: list[tuple[int, int]] = []
        for pattern, label in _TOOLS:
            for match in re.finditer(rf"\b{re.escape(pattern)}\b", text, flags=re.IGNORECASE):
                span = match.span()
                if any(span[0] < end and span[1] > start for start, end in occupied):
                    continue
                occupied.append(span)
                _add_mention(found, seen, "tool", label, at, segment.url)
    found.sort(key=lambda item: (item.kind, item.name.lower(), item.at))
    return found


def _mentions_in_plain(plain: str, vid: str, found: list[Mention]) -> list[Mention]:
    """Catch names that appear only in the untitled preamble (summary, tags)."""
    seen = {(item.kind, item.name.casefold()) for item in found}
    fake = [Segment(0, youtube_url(vid), plain, "", "")]
    extra = _mentions(fake)
    for item in extra:
        key = (item.kind, item.name.casefold())
        if key in seen:
            continue
        seen.add(key)
        found.append(Mention(item.kind, item.name, "", youtube_url(vid)))
    found.sort(key=lambda item: (item.kind, item.name.lower(), item.at))
    return found


def _add_mention(
    found: list[Mention],
    seen: set[tuple[str, str]],
    kind: str,
    name: str,
    at: str,
    url: str,
) -> None:
    key = (kind, name.casefold())
    if key in seen:
        return
    seen.add(key)
    found.append(Mention(kind, name, at, url))


def _quotes(segments: list[Segment]) -> list[Item]:
    transcript = [
        segment
        for segment in segments
        if "transcript" in segment.section.lower() or segment.heading.lower() == "full transcript"
    ]
    pool = transcript or segments
    picked: list[Item] = []
    seen: set[str] = set()
    for minimum, maximum in ((8, 28), (6, 40)):
        for segment in pool:
            words = segment.text.split()
            if not (minimum <= len(words) <= maximum):
                continue
            if segment.text.casefold() in seen:
                continue
            seen.add(segment.text.casefold())
            picked.append(Item(format_at(segment.start_s), segment.text, segment.url))
            if len(picked) >= 3 and minimum == 8:
                return picked
        if picked:
            return picked
    return picked


def known_timestamps(segments: list[Segment]) -> set[str]:
    return {format_at(segment.start_s) for segment in segments}


def merge_llm_claims(
    insights: Insights, payload: object, allowed: set[str], source: str
) -> Insights:
    """Keep model claims only when they quote the transcript at a real timestamp."""
    claims = _read_claims(payload)
    grounded: list[Item] = []
    for claim in claims:
        at = str(claim.get("at") or "")
        text = " ".join(str(claim.get("text") or "").split())
        if at not in allowed or len(text) < 20:
            continue
        if text.casefold() not in source.casefold():
            continue
        url = str(claim.get("url") or "")
        if not url:
            url = next((item.url for item in insights.claims if item.at == at), "")
        grounded.append(Item(at, text, url))
    if not grounded:
        return insights
    return Insights(
        chapters=insights.chapters,
        claims=grounded,
        mentions=insights.mentions,
        quotes=insights.quotes,
        chapters_total=insights.chapters_total,
        claims_total=len(grounded),
        mentions_total=insights.mentions_total,
        quotes_total=insights.quotes_total,
        method="llm",
    )


def _read_claims(payload: object) -> list[dict[str, object]]:
    if isinstance(payload, str):
        text = payload.strip()
        if text.startswith("```"):
            text = re.sub(r"^```(?:json)?\s*", "", text)
            text = re.sub(r"\s*```$", "", text)
        payload = json.loads(text)
    if not isinstance(payload, dict):
        return []
    claims = payload.get("claims")
    if not isinstance(claims, list):
        return []
    return [claim for claim in claims if isinstance(claim, dict)]


def post_llm(prompt: str) -> str:
    key = os.environ.get("HACKERTUBE_LLM_API_KEY", "")
    if not key:
        raise AppError("llm_unconfigured", "set HACKERTUBE_LLM_API_KEY or omit --llm")
    base = os.environ.get("HACKERTUBE_LLM_BASE_URL", "https://api.openai.com/v1").rstrip("/")
    model = os.environ.get("HACKERTUBE_LLM_MODEL", "gpt-4o-mini")
    body = json.dumps(
        {
            "model": model,
            "temperature": 0,
            "messages": [
                {
                    "role": "system",
                    "content": (
                        'Return JSON {"claims":[{"at":"m:ss","text":"verbatim quote"}]}. '
                        "Quote the transcript. Do not paraphrase. Use only timestamps that appear."
                    ),
                },
                {"role": "user", "content": prompt},
            ],
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        f"{base}/chat/completions",
        data=body,
        headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except Exception as exc:  # noqa: BLE001 — surface a structured error, not a traceback
        raise AppError("llm_failed", str(exc)) from exc
    try:
        content = payload["choices"][0]["message"]["content"]
    except (KeyError, IndexError, TypeError) as exc:
        raise AppError("llm_failed", "model response had no message content") from exc
    if not isinstance(content, str):
        raise AppError("llm_failed", "model content was not text")
    return content


def llm_prompt(vid: str, source_text: str) -> str:
    clipped = source_text[:12000]
    return (
        f"Video {vid}. Extract up to 5 grounded claims.\n"
        "Each claim text must be a verbatim substring. "
        "Each at must be a timestamp from the text.\n\n"
        f"{clipped}"
    )
