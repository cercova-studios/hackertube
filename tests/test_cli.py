import json
from pathlib import Path

from tests.conftest import run_cli, write_aietalks, write_dataset


def _ingest(home: Path) -> None:
    dataset = home / "dataset"
    talks = home / "talks"
    write_dataset(dataset)
    write_aietalks(talks)
    first = run_cli(home, ["ingest", "--source", "hackertube", "--path", str(dataset)])
    assert first.returncode == 0, first.stdout + first.stderr
    assert "hackertube,7,6,true" in first.stdout
    second = run_cli(home, ["ingest", "--source", "hackertube", "--path", str(dataset)])
    assert second.returncode == 0, second.stdout
    assert "hackertube,7,6,false" in second.stdout
    assert "catalog unchanged" in second.stdout
    talks_run = run_cli(home, ["ingest", "--source", "aietalks", "--path", str(talks)])
    assert talks_run.returncode == 0, talks_run.stdout + talks_run.stderr


def test_home_is_content_and_has_help(tmp_path: Path) -> None:
    home = tmp_path / "home"
    _ingest(home)
    result = run_cli(home, [])
    assert result.returncode == 0, result.stdout
    assert "description:" in result.stdout
    assert "bin:" in result.stdout
    assert "state:" in result.stdout
    assert "help[" in result.stdout
    assert "picks[" in result.stdout
    assert result.stderr == ""


def test_recommend_rank_and_filters(tmp_path: Path) -> None:
    home = tmp_path / "home"
    _ingest(home)
    agentic = run_cli(home, ["recommend", "--area", "agentic-engineering", "--limit", "5"])
    assert agentic.returncode == 0, agentic.stdout
    assert "videos[" in agentic.stdout
    assert "{vid,title,score,reason}:" in agentic.stdout
    assert "abcdefghijk" in agentic.stdout
    assert "ccccccccccc" in agentic.stdout
    assert "fffffffffff" not in agentic.stdout
    assert "ddddddddddd" not in agentic.stdout
    # The AI Engineer talk outranks the lower-scored HackerTube demo.
    assert agentic.stdout.index("abcdefghijk") < agentic.stdout.index("ccccccccccc")

    engineering = run_cli(home, ["recommend", "--area", "ai-engineering"])
    assert "aaaaaaaaaaa" in engineering.stdout
    assert "eeeeeeeeeee" not in engineering.stdout
    assert "ddddddddddd" not in engineering.stdout

    with_news = run_cli(home, ["recommend", "--area", "ai-engineering", "--include-news"])
    assert "eeeeeeeeeee" in with_news.stdout

    research = run_cli(home, ["recommend", "--area", "ai-research"])
    assert research.stdout.index("bbbbbbbbbbb") < research.stdout.index("ggggggggggg")


def test_state_exclusions_are_idempotent(tmp_path: Path) -> None:
    home = tmp_path / "home"
    _ingest(home)
    seen = run_cli(home, ["seen", "aaaaaaaaaaa"])
    assert seen.returncode == 0
    assert "changed: true" in seen.stdout
    again = run_cli(home, ["seen", "aaaaaaaaaaa"])
    assert again.returncode == 0
    assert "changed: false" in again.stdout
    ranked = run_cli(home, ["recommend", "--area", "ai-engineering"])
    assert "aaaaaaaaaaa" not in ranked.stdout
    assert "ggggggggggg" in ranked.stdout

    liked = run_cli(home, ["like", "ggggggggggg"])
    assert liked.returncode == 0
    still = run_cli(home, ["recommend", "--area", "ai-engineering"])
    assert "ggggggggggg" in still.stdout

    run_cli(home, ["dislike", "ggggggggggg"])
    hidden = run_cli(home, ["recommend", "--area", "ai-engineering"])
    assert "ggggggggggg" not in hidden.stdout
    assert "empty:" in hidden.stdout

    run_cli(home, ["forget", "aaaaaaaaaaa"])
    run_cli(home, ["forget", "ggggggggggg"])
    restored = run_cli(home, ["recommend", "--area", "ai-engineering"])
    assert "aaaaaaaaaaa" in restored.stdout

    run_cli(home, ["snooze", "aaaaaaaaaaa", "--until", "2026-11-11"])
    snoozed = run_cli(home, ["recommend", "--area", "ai-engineering"])
    assert "aaaaaaaaaaa" not in snoozed.stdout
    run_cli(home, ["snooze", "aaaaaaaaaaa", "--until", "2026-11-10"])
    expired = run_cli(home, ["recommend", "--area", "ai-engineering"])
    assert "aaaaaaaaaaa" in expired.stdout


def test_search_similar_and_contract(tmp_path: Path) -> None:
    home = tmp_path / "home"
    _ingest(home)
    found = run_cli(home, ["search", "GPT walkthrough", "--fields", "vid,title"])
    assert found.returncode == 0, found.stdout
    assert "{vid,title}:" in found.stdout
    assert "aaaaaaaaaaa" in found.stdout
    assert "semantic: off" in found.stdout

    empty = run_cli(home, ["search", "zzzz-not-a-topic"])
    assert empty.returncode == 0
    assert "empty:" in empty.stdout
    assert "videos[0]" in empty.stdout

    payload = json.loads(run_cli(home, ["search", "GPT", "--json"]).stdout)
    assert isinstance(payload["videos"], list)
    assert payload["help"]

    similar = run_cli(home, ["similar", "aaaaaaaaaaa"])
    assert similar.returncode == 0, similar.stdout
    assert "mode: embedding" in similar.stdout
    assert similar.stdout.index("ccccccccccc") < similar.stdout.index("bbbbbbbbbbb")

    comedy = run_cli(home, ["similar", "ddddddddddd"])
    assert "mode: co-recommended" in comedy.stdout
    assert "aaaaaaaaaaa" in comedy.stdout

    segments = run_cli(home, ["search", "stagehand", "--in", "transcripts"])
    assert segments.returncode == 0, segments.stdout
    assert "abcdefghijk" in segments.stdout
    assert "2:10" in segments.stdout


def test_transcript_and_insights(tmp_path: Path) -> None:
    home = tmp_path / "home"
    _ingest(home)
    transcript = run_cli(home, ["transcript", "abcdefghijk"])
    assert transcript.returncode == 0, transcript.stdout
    assert "origin: aietalks" in transcript.stdout
    assert "showing:" in transcript.stdout
    assert "…[+" in transcript.stdout
    full = run_cli(home, ["transcript", "abcdefghijk", "--full"])
    assert "…[+" not in full.stdout
    assert "Source: https://www.youtube.com/watch?v=abcdefghijk" in full.stdout

    insights = run_cli(home, ["insights", "abcdefghijk"])
    assert insights.returncode == 0, insights.stdout
    assert "method: deterministic" in insights.stdout
    assert "claims[" in insights.stdout
    assert "mentions[" in insights.stdout
    assert "github.com/browserbase/stagehand" in insights.stdout
    assert "arxiv:2406.01234" in insights.stdout
    assert "t=130" in insights.stdout or "t=65" in insights.stdout

    missing = run_cli(home, ["transcript", "aaaaaaaaaaa"])
    assert missing.returncode == 1
    assert "transcript_unavailable" in missing.stdout
    assert missing.stdout.startswith("ok:") or "ok: false" in missing.stdout

    llm = run_cli(home, ["insights", "abcdefghijk", "--llm"])
    assert llm.returncode == 1
    assert "llm_unconfigured" in llm.stdout


def test_exit_codes_and_setup(tmp_path: Path) -> None:
    home = tmp_path / "home"
    home.mkdir()
    unknown = run_cli(home, ["--not-a-flag"])
    assert unknown.returncode == 2
    assert "unknown_flag" in unknown.stdout
    assert "exit: 2" in unknown.stdout

    bad_area = run_cli(home, ["recommend", "--area", "cooking"])
    assert bad_area.returncode == 2
    assert "error:" in bad_area.stdout

    bad_vid = run_cli(home, ["show", "nope"])
    assert bad_vid.returncode == 2

    _ingest(home)
    missing = run_cli(home, ["show", "zzzzzzzzzzz"])
    assert missing.returncode == 1
    assert "not_found" in missing.stdout

    help_text = run_cli(home, ["recommend", "--help"])
    assert help_text.returncode == 0
    assert "focus area" in help_text.stdout

    setup = run_cli(home, ["setup"])
    assert setup.returncode == 0, setup.stdout
    assert (home / "hooks" / "session.sh").exists()
    settings = json.loads((home / ".claude" / "settings.json").read_text(encoding="utf-8"))
    assert settings["hooks"]["SessionStart"][0]["hooks"][0]["command"] == "hackertube"
    again = run_cli(home, ["setup"])
    assert "changed: false" in again.stdout

    areas = run_cli(home, ["areas"])
    assert "agentic-engineering" in areas.stdout
    assert "ai-engineering" in areas.stdout
    assert "ai-research" in areas.stdout
