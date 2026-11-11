"""384-d BAAI/bge-small-en-v1.5 vectors, L2-normalized, no query prefix.

HackerTube's shipped matrix is adopted as-is. New texts (AI Engineer talks and
search queries) use fastembed's TextEmbedding when that extra is installed.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import numpy as np

from hackertube.errors import AppError

DIM = 384
MODEL_NAME = "BAAI/bge-small-en-v1.5"

try:
    from fastembed import TextEmbedding
except ImportError:  # optional extra: uv sync --extra embed
    TextEmbedding = None

_model: object | None = None
_model_failed: str | None = None


def matrix_path(home: Path) -> Path:
    return home / "embeddings.npy"


def load_matrix(home: Path) -> np.ndarray:
    path = matrix_path(home)
    if not path.exists():
        return np.zeros((0, DIM), dtype=np.float32)
    matrix = np.load(path)
    if matrix.ndim != 2 or matrix.shape[1] != DIM:
        raise AppError("bad_embeddings", f"expected (*, {DIM}) matrix in {path}")
    return matrix.astype(np.float32)


def save_matrix(home: Path, matrix: np.ndarray) -> None:
    home.mkdir(parents=True, exist_ok=True)
    np.save(matrix_path(home), np.asarray(matrix, dtype=np.float16))


def normalize(matrix: np.ndarray) -> np.ndarray:
    vectors = np.asarray(matrix, dtype=np.float32)
    if vectors.ndim == 1:
        vectors = vectors.reshape(1, -1)
    norms = np.linalg.norm(vectors, axis=1, keepdims=True)
    return vectors / np.clip(norms, 1e-12, None)


def adopt(home: Path, npy_path: Path, ids: list[str], current: dict[str, int]) -> dict[str, int]:
    """Merge a precomputed matrix into the local store, keyed by video id."""
    incoming = np.load(npy_path)
    if incoming.ndim != 2 or incoming.shape[1] != DIM or incoming.shape[0] != len(ids):
        raise AppError(
            "bad_embeddings",
            f"{npy_path.name} has shape {getattr(incoming, 'shape', None)} for {len(ids)} ids",
        )
    vectors = normalize(incoming.astype(np.float32))
    return merge_rows(home, list(zip(ids, vectors, strict=True)), current)


def merge_rows(
    home: Path,
    rows: list[tuple[str, np.ndarray]],
    current: dict[str, int],
) -> dict[str, int]:
    existing = load_matrix(home)
    fresh = sum(1 for vid, _vector in rows if vid not in current)
    total = existing.shape[0] + fresh
    merged = np.zeros((total, DIM), dtype=np.float32)
    if existing.shape[0]:
        merged[: existing.shape[0]] = existing
    mapping = dict(current)
    cursor = existing.shape[0]
    for vid, vector in rows:
        row = normalize(np.asarray(vector, dtype=np.float32).reshape(1, -1))[0]
        if vid in mapping:
            merged[mapping[vid]] = row
        else:
            mapping[vid] = cursor
            merged[cursor] = row
            cursor += 1
    save_matrix(home, merged[:cursor])
    return mapping


def cast_embed(embedder: object, texts: list[str]) -> Any:
    embed = getattr(embedder, "embed", None)
    if embed is None:
        raise AppError("embedder_missing", "fastembed model has no embed method")
    return embed(texts)


def embed_texts(texts: list[str]) -> np.ndarray | None:
    """Return normalized vectors, or None when fastembed is unavailable."""
    global _model, _model_failed
    if _model_failed:
        return None
    if not texts:
        return np.zeros((0, DIM), dtype=np.float32)
    if TextEmbedding is None:
        return None
    try:
        if _model is None:
            _model = TextEmbedding(MODEL_NAME)
        raw = list(cast_embed(_model, texts))
    except Exception as exc:  # noqa: BLE001 — model download or runtime failure
        _model_failed = str(exc)
        _model = None
        return None
    matrix = np.vstack([np.asarray(vector, dtype=np.float32) for vector in raw])
    return normalize(matrix)


def embed_status() -> str:
    if _model_failed or TextEmbedding is None:
        return "off"
    return "fastembed"


def semantic_scores(home: Path, query: str, mapping: dict[str, int]) -> dict[str, float] | None:
    if not mapping:
        return {}
    vectors = embed_texts([query])
    if vectors is None:
        return None
    matrix = load_matrix(home)
    if matrix.shape[0] == 0:
        return {}
    query_vector = vectors[0]
    scores = matrix @ query_vector
    by_vid: dict[str, float] = {}
    for vid, row in mapping.items():
        if 0 <= row < scores.shape[0]:
            by_vid[vid] = float(scores[row])
    return by_vid
