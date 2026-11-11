"""Load the ODC-By HackerTube dataset from a local directory or zip.

The raw zip is never part of this repository. ``ingest`` downloads it or
accepts a path the agent already has.
"""

from __future__ import annotations

import hashlib
import json
import urllib.request
import zipfile
from pathlib import Path

from hackertube.db import (
    embed_map,
    meta_get,
    meta_set,
    replace_mentions,
    replace_related,
    set_embed_row,
    upsert_video,
)
from hackertube.embed import adopt
from hackertube.errors import AppError
from hackertube.paths import now_iso
from hackertube.sources.base import IngestContext, SourceResult

DATASET_URL = "https://hackertube.org/download/hackertube-data.zip"
_MAX_ZIP_BYTES = 200_000_000


def ingest_hackertube(ctx: IngestContext) -> SourceResult:
    root = resolve_dataset(ctx)
    videos_path = root / "videos.jsonl"
    digest = _fingerprint(root)
    previous = meta_get(ctx.conn, "hackertube_sha256")
    existing = ctx.conn.execute(
        "SELECT COUNT(*) AS n FROM videos WHERE source = 'hackertube'"
    ).fetchone()
    have = int(existing["n"]) if existing is not None else 0
    if previous == digest and have and not ctx.refresh:
        embedded_row = ctx.conn.execute(
            "SELECT COUNT(*) AS n FROM videos WHERE source = 'hackertube' AND embed_row IS NOT NULL"
        ).fetchone()
        embedded = int(embedded_row["n"]) if embedded_row is not None else 0
        return SourceResult("hackertube", have, embedded, False, "catalog unchanged")
    records = [
        _video_record(json.loads(line))
        for line in videos_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    vids = [str(record["vid"]) for record in records]
    for record in records:
        record["updated_at"] = now_iso()
        upsert_video(ctx.conn, record)
    mention_rows = _mentions(root)
    if mention_rows:
        replace_mentions(ctx.conn, vids, mention_rows)
    related_rows = _related(root)
    if related_rows:
        replace_related(ctx.conn, related_rows)
    embedded = _embeddings(ctx, root)
    meta_set(ctx.conn, "hackertube_sha256", digest)
    meta_set(ctx.conn, "hackertube_root", str(root))
    return SourceResult(
        "hackertube",
        len(records),
        embedded,
        True,
        f"loaded {len(records)} videos from {root.name}",
    )


def resolve_dataset(ctx: IngestContext) -> Path:
    if ctx.path is not None:
        return _root_from_path(ctx.path)
    cache = ctx.home / "cache"
    cache.mkdir(parents=True, exist_ok=True)
    zip_path = cache / "hackertube-data.zip"
    if ctx.refresh or not zip_path.exists():
        _download(DATASET_URL, zip_path)
    extracted = cache / "hackertube-data"
    if ctx.refresh or not (extracted / "videos.jsonl").exists():
        if extracted.exists():
            # Leave a previous extract in place until the new zip is valid.
            pass
        _safe_extract(zip_path, extracted)
    return _root_from_path(extracted)


def _root_from_path(path: Path) -> Path:
    if path.is_file() and path.suffix == ".zip":
        dest = path.parent / f"{path.stem}-extracted"
        _safe_extract(path, dest)
        path = dest
    if not path.is_dir():
        raise AppError("dataset_path", f"{path} is not a directory or zip")
    if (path / "videos.jsonl").exists():
        return path
    for child in sorted(path.iterdir()):
        if child.is_dir() and (child / "videos.jsonl").exists():
            return child
    raise AppError("dataset_path", f"no videos.jsonl under {path}")


def _safe_extract(zip_path: Path, dest: Path) -> None:
    dest.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(zip_path) as archive:
        for info in archive.infolist():
            parts = Path(info.filename).parts
            if info.filename.startswith("/") or ".." in parts:
                raise AppError("unsafe_zip", f"refusing zip member {info.filename}")
        archive.extractall(dest)


def _download(url: str, dest: Path) -> None:
    request = urllib.request.Request(url, headers={"User-Agent": "hackertube/0.1"})
    partial = dest.with_suffix(".partial")
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            size = 0
            with partial.open("wb") as handle:
                while True:
                    chunk = response.read(1024 * 256)
                    if not chunk:
                        break
                    size += len(chunk)
                    if size > _MAX_ZIP_BYTES:
                        raise AppError(
                            "download_too_large", f"{url} exceeded {_MAX_ZIP_BYTES} bytes"
                        )
                    handle.write(chunk)
    except AppError:
        partial.unlink(missing_ok=True)
        raise
    except Exception as exc:  # noqa: BLE001
        partial.unlink(missing_ok=True)
        raise AppError("download_failed", f"could not download {url}: {exc}") from exc
    partial.replace(dest)


def _fingerprint(root: Path) -> str:
    digest = hashlib.sha256()
    for name in ("videos.jsonl", "embeddings.npy", "embedding_ids.json"):
        file = root / name
        if not file.exists():
            continue
        digest.update(name.encode())
        digest.update(file.read_bytes())
    return digest.hexdigest()


def _as_float(value: object) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float, str)):
        return 0.0
    return float(value)


def _video_record(row: dict[str, object]) -> dict[str, object]:
    categories = row.get("categories") or []
    category_ids: list[str] = []
    if isinstance(categories, list):
        for category in categories:
            if isinstance(category, dict) and category.get("id"):
                category_ids.append(str(category["id"]))
    quote = row.get("quote") or {}
    quote_text = ""
    if isinstance(quote, dict) and quote.get("text"):
        quote_text = str(quote["text"])
    hn = row.get("hn") or {}
    recommenders = 0
    if isinstance(hn, dict):
        recommenders = int(hn.get("recommenders") or 0)
    tags = row.get("tags") if isinstance(row.get("tags"), list) else []
    return {
        "vid": str(row["vid"]),
        "source": "hackertube",
        "url": str(row.get("url") or f"https://www.youtube.com/watch?v={row['vid']}"),
        "page_url": "https://hackertube.org",
        "title": str(row.get("title") or ""),
        "channel": str(row.get("channel") or ""),
        "summary": str(row.get("summary") or ""),
        "why": str(row.get("why_hn") or ""),
        "content_type": str(row.get("content_type") or ""),
        "level": str(row.get("level") or ""),
        "shelf_life": str(row.get("shelf_life") or ""),
        "primary_topic": str(row.get("primary_topic") or ""),
        "tags_json": json.dumps(tags),
        "categories_json": json.dumps(category_ids),
        "topics_json": "[]",
        "quote": quote_text,
        "hn_recommenders": recommenders,
        "score": _as_float(row.get("score")),
        "published": "",
        "event": "",
        "transcript_path": "",
        "embed_row": None,
        "updated_at": "",
    }


def _mentions(root: Path) -> list[tuple[object, ...]]:
    path = root / "mentions.jsonl"
    if not path.exists():
        return []
    rows: list[tuple[object, ...]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        item = json.loads(line)
        rows.append(
            (
                int(item["hn_comment_id"]),
                str(item["vid"]),
                str(item.get("hn_url") or ""),
                str(item.get("stance") or ""),
                str(item.get("why") or ""),
                str(item.get("quote") or ""),
                str(item.get("month") or ""),
            )
        )
    return rows


def _related(root: Path) -> list[tuple[object, ...]]:
    path = root / "related.jsonl"
    if not path.exists():
        return []
    rows: list[tuple[object, ...]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        item = json.loads(line)
        vid = str(item["vid"])
        related = item.get("related") or []
        if not isinstance(related, list):
            continue
        for other in related:
            if isinstance(other, dict) and other.get("vid"):
                rows.append((vid, str(other["vid"]), int(other.get("shared_recommenders") or 0)))
    return rows


def _embeddings(ctx: IngestContext, root: Path) -> int:
    npy = root / "embeddings.npy"
    ids_path = root / "embedding_ids.json"
    if not npy.exists() or not ids_path.exists():
        return 0
    ids = json.loads(ids_path.read_text(encoding="utf-8"))
    if not isinstance(ids, list):
        raise AppError("bad_embeddings", "embedding_ids.json must be a list")
    mapping = adopt(ctx.home, npy, [str(vid) for vid in ids], embed_map(ctx.conn))
    for vid, row in mapping.items():
        if vid in set(ids):
            set_embed_row(ctx.conn, vid, row)
    return len(ids)
