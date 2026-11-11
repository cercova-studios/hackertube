"""Offline fixtures. Tests must not open the network."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import urllib.request
from pathlib import Path

import numpy as np
import pytest

ROOT = Path(__file__).resolve().parents[1]
TALK = Path(__file__).parent / "fixtures" / "talk.md"


@pytest.fixture(autouse=True)
def _no_network(monkeypatch: pytest.MonkeyPatch) -> None:
    def blocked(*_args: object, **_kwargs: object) -> None:
        raise AssertionError("network disabled in tests")

    monkeypatch.setattr(urllib.request, "urlopen", blocked)


def run_cli(
    home: Path,
    args: list[str],
    *,
    extra_env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["HACKERTUBE_HOME"] = str(home)
    env["HOME"] = str(home)
    env["HACKERTUBE_TODAY"] = "2026-11-11"
    env["PYTHONPATH"] = str(ROOT / "src")
    env.pop("YOUTUBE_API_KEY", None)
    env.pop("HACKERTUBE_LLM_API_KEY", None)
    if extra_env:
        env.update(extra_env)
    return subprocess.run(
        [sys.executable, "-m", "hackertube", *args],
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )


def write_dataset(root: Path) -> None:
    videos = [
        _video(
            "aaaaaaaaaaa",
            "Let's build GPT",
            "Andrej Karpathy",
            "A spelled-out language model from scratch.",
            "HN's favorite GPT walkthrough.",
            "tutorial",
            "evergreen",
            "language models",
            ["gpt", "llm"],
            ["llms-generative-ai"],
            10,
            5,
        ),
        _video(
            "bbbbbbbbbbb",
            "AI safety alignment",
            "Researcher",
            "A lecture on AI safety and alignment.",
            "A clear alignment lecture.",
            "lecture",
            "evergreen",
            "ai safety",
            ["alignment"],
            ["ai-safety-impact"],
            8,
            4,
        ),
        _video(
            "ccccccccccc",
            "Vibe coding with Claude Code",
            "Builder",
            "A coding agent edits a Rust repo with Claude Code.",
            "Shows a coding agent editing a repo.",
            "demo",
            "evergreen",
            "coding agents",
            ["claude code", "vibe coding"],
            ["software-practice-careers"],
            3,
            2,
        ),
        _video(
            "ddddddddddd",
            "Monty Python sketch",
            "Monty Python",
            "A comedy sketch.",
            "Quoted for the joke.",
            "comedy",
            "evergreen",
            "comedy",
            ["sketch"],
            ["standup-sketch-comedy"],
            20,
            10,
        ),
        _video(
            "eeeeeeeeeee",
            "News model launch",
            "Wire",
            "A one-day model announcement.",
            "Only matters this week.",
            "news",
            "news",
            "llm",
            ["llm"],
            ["llms-generative-ai"],
            12,
            3,
        ),
        _video(
            "fffffffffff",
            "Primitive Technology: Stone Axe",
            "Primitive Technology",
            "Tool use in the wild while making a stone axe.",
            "A practical demonstration.",
            "documentary",
            "evergreen",
            "stone tools",
            ["tool use"],
            ["home-diy-repair"],
            6,
            4,
        ),
        _video(
            "ggggggggggg",
            "Backpropagation lecture",
            "University",
            "A neural network backpropagation lecture.",
            "The standard backprop derivation.",
            "lecture",
            "evergreen",
            "neural networks",
            ["backpropagation"],
            ["machine-learning"],
            7,
            3,
        ),
    ]
    root.mkdir(parents=True, exist_ok=True)
    (root / "videos.jsonl").write_text(
        "".join(json.dumps(video) + "\n" for video in videos), encoding="utf-8"
    )
    ids = [video["vid"] for video in videos if video["vid"] != "ddddddddddd"]
    matrix = np.zeros((len(ids), 384), dtype=np.float32)
    hot = {vid: index for index, vid in enumerate(ids)}
    for _vid, index in hot.items():
        matrix[index, index % 384] = 1.0
    # Pull the Claude Code talk toward the GPT talk so similar(a) returns c.
    matrix[hot["ccccccccccc"]] = 0
    matrix[hot["ccccccccccc"], 0] = 0.8
    matrix[hot["ccccccccccc"], 2] = 0.2
    norms = np.linalg.norm(matrix, axis=1, keepdims=True)
    matrix = matrix / np.clip(norms, 1e-12, None)
    np.save(root / "embeddings.npy", matrix.astype(np.float16))
    (root / "embedding_ids.json").write_text(json.dumps(ids), encoding="utf-8")
    mention = {
        "hn_comment_id": 1,
        "vid": "aaaaaaaaaaa",
        "hn_url": "https://news.ycombinator.com/item?id=1",
        "stance": "strong_rec",
        "why": "Spelled-out GPT",
        "quote": "spelled out",
        "month": "2023-01",
    }
    (root / "mentions.jsonl").write_text(json.dumps(mention) + "\n", encoding="utf-8")
    related = {
        "vid": "ddddddddddd",
        "related": [{"vid": "aaaaaaaaaaa", "shared_recommenders": 2}],
    }
    (root / "related.jsonl").write_text(json.dumps(related) + "\n", encoding="utf-8")


def write_aietalks(root: Path) -> None:
    root.mkdir(parents=True, exist_ok=True)
    markdown = root / "markdown"
    markdown.mkdir()
    (markdown / "abcdefghijk.md").write_text(TALK.read_text(encoding="utf-8"), encoding="utf-8")
    talks = [
        {
            "slug": "example-talk",
            "title": "Agents Need a Harness",
            "url": "https://ai.engineer/talks/abcdefghijk-example-talk",
            "videoId": "abcdefghijk",
            "event": "AI Engineer 2026",
            "speakers": [{"name": "Ada Lovelace"}],
            "topics": [
                {"slug": "coding-agents", "name": "Coding agents"},
                {"slug": "agent-engineering", "name": "Agent engineering"},
            ],
            "summary": (
                "Ada argues a coding agent should be graded on completed work. "
                "The talk covers MCP and Claude Code."
            ),
        }
    ]
    transcripts = [
        {
            "talkSlug": "example-talk",
            "title": "Agents Need a Harness",
            "jsonUrl": "https://ai.engineer/api/data/transcript?slug=example-talk&format=json",
            "csvUrl": "https://ai.engineer/api/data/transcript?slug=example-talk&format=csv",
        }
    ]
    (root / "talks.json").write_text(json.dumps(talks), encoding="utf-8")
    (root / "transcripts.json").write_text(json.dumps(transcripts), encoding="utf-8")


def _video(
    vid: str,
    title: str,
    channel: str,
    summary: str,
    why: str,
    content_type: str,
    shelf_life: str,
    topic: str,
    tags: list[str],
    categories: list[str],
    score: float,
    recommenders: int,
) -> dict[str, object]:
    return {
        "vid": vid,
        "url": f"https://www.youtube.com/watch?v={vid}",
        "title": title,
        "channel": channel,
        "summary": summary,
        "why_hn": why,
        "content_type": content_type,
        "level": "intermediate",
        "shelf_life": shelf_life,
        "primary_topic": topic,
        "tags": tags,
        "categories": [
            {"id": category, "name": category, "area": "ai-data", "area_name": "AI"}
            for category in categories
        ],
        "quote": {"text": why, "hn_comment_id": 1, "year": "2024"},
        "hn": {
            "recommenders": recommenders,
            "mentions": recommenders,
            "rec_months": recommenders,
            "strong_recs": 1,
            "stances": {"rec": recommenders},
            "first_year": 2023,
            "last_year": 2026,
        },
        "score": score,
    }
