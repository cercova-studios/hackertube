"""Focus areas for an AI/ML engineer. Rules are data, not a model call.

A video can sit in more than one area. Affinity is an integer so ties fall
through to the stored score and then the video id.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

from hackertube.models import Video

AREA_IDS = ("agentic-engineering", "ai-engineering", "ai-research")

_EXCLUDED_TYPES = frozenset({"comedy", "music"})
_AI_CONTEXT = re.compile(
    r"\b(ai|llm|gpt|chatgpt|claude|agentic|mcp|transformer|neural|model|coding)\b",
    re.IGNORECASE,
)
_NEGATIVE_TITLE = re.compile(
    r"(transformer home|electrical transformer|stone axe|ableton|academy award|hyperland)",
    re.IGNORECASE,
)
_ML_PHRASE = re.compile(
    r"(language model|\bllm\b|chatgpt|gpt-4|gpt-3|transformer|neural network|"
    r"deep learning|diffusion|fine-?tun|retrieval-augmented|\brag\b|backprop)",
    re.IGNORECASE,
)


@dataclass(frozen=True)
class Area:
    id: str
    description: str
    categories: frozenset[str]
    topic_slugs: frozenset[str]
    strong_phrases: tuple[str, ...]
    context_phrases: tuple[str, ...]


AREAS: dict[str, Area] = {
    "agentic-engineering": Area(
        id="agentic-engineering",
        description="Coding agents, harnesses, MCP, tool calling, and agent skills",
        categories=frozenset(),
        topic_slugs=frozenset(
            {
                "coding-agents",
                "agent-engineering",
                "model-context-protocol",
                "harness-engineering",
                "agent-memory",
                "agent-skills",
                "agent-orchestration",
                "multi-agent-systems",
                "computer-use",
                "workflow-automation",
                "software-factories",
                "context-engineering",
                "sandboxes-and-execution-isolation",
            }
        ),
        strong_phrases=(
            "coding agent",
            "claude code",
            "vibe coding",
            "swe-agent",
            "agent harness",
            "model context protocol",
            "agentic",
            "llm agent",
        ),
        context_phrases=("ai agent", "multi-agent", "function calling", "tool calling"),
    ),
    "ai-engineering": Area(
        id="ai-engineering",
        description="LLMs, RAG, evals, fine-tuning, inference, and applied model building",
        categories=frozenset({"llms-generative-ai"}),
        topic_slugs=frozenset(
            {
                "rag-and-knowledge",
                "evals",
                "fine-tuning",
                "inference",
                "local-inference",
                "search-and-retrieval",
                "structured-outputs",
                "observability-and-reliability",
                "developer-workflows-and-testing",
                "ai-cost-and-performance-engineering",
                "post-training",
                "voice-ai",
            }
        ),
        strong_phrases=(
            "large language model",
            "language model",
            "chatgpt",
            "retrieval-augmented",
            "prompt engineering",
            "fine-tuning",
            "fine tuning",
        ),
        context_phrases=("transformer", "gpt-4", "gpt-3", "llm"),
    ),
    "ai-research": Area(
        id="ai-research",
        description="Model research, alignment, safety, and lecture-depth ML",
        categories=frozenset({"ai-safety-impact"}),
        topic_slugs=frozenset(
            {
                "safety-and-governance",
                "machine-learning-fundamentals",
                "ai-security",
                "data-quality-and-curation",
            }
        ),
        strong_phrases=(
            "ai safety",
            "alignment",
            "scaling law",
            "attention is all you need",
            "backpropagation",
        ),
        context_phrases=("neural network", "deep learning", "arxiv"),
    ),
}


def parse_area(value: str | None) -> str | None:
    if value is None:
        return None
    key = value.strip().lower()
    if key not in AREAS:
        known = ", ".join(AREA_IDS)
        raise ValueError(known)
    return key


def affinity(video: Video, area_id: str) -> int:
    """0 means out of area. 2 is a usable match. 3 is a direct category or phrase hit."""
    if video.content_type in _EXCLUDED_TYPES:
        return 0
    if _NEGATIVE_TITLE.search(video.title):
        return 0
    area = AREAS[area_id]
    blob = video.blob()
    score = 0
    categories = set(video.category_ids)
    if categories & area.categories:
        score = max(score, 3)
    if "machine-learning" in categories and area_id == "ai-engineering" and _ML_PHRASE.search(blob):
        score = max(score, 2)
    if (
        area_id == "ai-research"
        and "machine-learning" in categories
        and video.content_type in {"lecture", "talk", "course"}
        and _ML_PHRASE.search(blob)
    ):
        score = max(score, 2)
    if set(video.topic_slugs) & area.topic_slugs:
        score = max(score, 3)
    if any(phrase in blob for phrase in area.strong_phrases):
        score = max(score, 3)
    if any(phrase in blob for phrase in area.context_phrases) and (
        _AI_CONTEXT.search(blob)
        or categories & {"llms-generative-ai", "machine-learning", "ai-safety-impact"}
    ):
        score = max(score, 2)
    return score


def matching_areas(video: Video) -> list[str]:
    return [area_id for area_id in AREA_IDS if affinity(video, area_id) >= 2]


def reason_for(video: Video, area_id: str) -> str:
    """One line copied from stored fields. Nothing is invented."""
    if video.why.strip():
        return _sentence(video.why)
    if video.summary.strip():
        return _sentence(video.summary)
    if video.topic_slugs:
        shown = ", ".join(video.topic_slugs[:3])
        return f"AI Engineer topics: {shown}"
    if video.primary_topic:
        return f"Topic: {video.primary_topic}"
    return f"Matches {area_id}"


def _sentence(text: str) -> str:
    cleaned = " ".join(text.split())
    for mark in (". ", "? ", "! "):
        if mark in cleaned:
            return cleaned.split(mark, 1)[0] + mark.strip()
    return cleaned
