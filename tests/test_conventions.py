from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_agent_docs_and_skills_exist() -> None:
    assert (ROOT / "AGENTS.md").exists()
    assert (ROOT / "NOTICE").exists()
    notice = (ROOT / "NOTICE").read_text(encoding="utf-8")
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    assert "ODC-By" in notice
    assert "hackertube.org" in notice
    assert "ODC-By" in readme
    assert "hackertube.org" in readme
    skills = ["contributing", "add-source", "add-command", "hackertube"]
    for name in skills:
        skill = ROOT / ".agents" / "skills" / name / "SKILL.md"
        text = skill.read_text(encoding="utf-8")
        assert text.startswith("---\n")
        assert f"name: {name}" in text
        for harness in (".cursor", ".claude"):
            link = ROOT / harness / "skills" / name
            assert link.is_symlink()
            assert (link / "SKILL.md").exists()
    adrs = list((ROOT / "docs" / "adr").glob("*.md"))
    assert len(adrs) >= 5
