from pathlib import Path

from hackertube.insights import extract, merge_llm_claims, parse_markdown, parse_timestamp
from hackertube.transcript import parse_identity

TALK = Path(__file__).parent / "fixtures" / "talk.md"
VID = "abcdefghijk"


def test_identity_reads_source_and_published() -> None:
    vid, published = parse_identity(TALK.read_text(encoding="utf-8"))
    assert vid == VID
    assert published == "2026-01-15"


def test_extracts_claims_repo_paper_and_quotes() -> None:
    text = TALK.read_text(encoding="utf-8")
    segments = parse_markdown(text, VID)
    insights = extract(segments, plain=text, vid=VID)
    assert any(item.text.startswith("Agents need a harness") for item in insights.chapters)
    assert any("graded on completed work" in item.text for item in insights.claims)
    assert all(
        item.url.startswith("https://www.youtube.com/watch?v=abcdefghijk")
        for item in insights.claims
    )
    assert all(item.at for item in insights.claims)
    names = {item.name for item in insights.mentions}
    assert "github.com/browserbase/stagehand" in names
    assert "arxiv:2406.01234" in names
    assert "Claude Code" in names
    assert "Stagehand" in names
    assert "MCP" in names
    assert any("completed work" in item.text for item in insights.quotes)
    repo = next(item for item in insights.mentions if item.kind == "repo")
    assert repo.at == "2:10"
    assert "t=130" in repo.url


def test_llm_merge_keeps_only_verbatim_timestamps() -> None:
    text = TALK.read_text(encoding="utf-8")
    segments = parse_markdown(text, VID)
    insights = extract(segments, plain=text, vid=VID)
    verbatim = "A coding agent should be graded on completed work, not a single answer."
    merged = merge_llm_claims(
        insights,
        {
            "claims": [
                {"at": "1:05", "text": verbatim},
                {"at": "9:99", "text": verbatim},
                {"at": "1:05", "text": "This paraphrase is not in the transcript."},
            ]
        },
        {"1:05", "2:10", "0:00"},
        text,
    )
    assert merged.method == "llm"
    assert len(merged.claims) == 1
    assert merged.claims[0].text == verbatim


def test_timestamp_parser() -> None:
    assert parse_timestamp("1:05") == 65
    assert parse_timestamp("01:02:03") == 3723
