from hackertube.areas import affinity
from hackertube.models import Video


def _video(**kwargs: object) -> Video:
    base: dict[str, object] = {
        "vid": "aaaaaaaaaaa",
        "source": "hackertube",
        "url": "https://www.youtube.com/watch?v=aaaaaaaaaaa",
        "page_url": "",
        "title": "Untitled",
        "channel": "",
        "summary": "",
        "why": "",
        "content_type": "talk",
        "level": "general",
        "shelf_life": "evergreen",
        "primary_topic": "",
        "tags": (),
        "category_ids": (),
        "topic_slugs": (),
        "score": 1,
        "published": "",
        "event": "",
        "hn_recommenders": 1,
        "quote": "",
        "embed_row": None,
        "transcript_path": "",
    }
    base.update(kwargs)
    return Video(**base)  # type: ignore[arg-type]


def test_agentic_phrase_matches_and_stone_axe_does_not() -> None:
    coding = _video(title="Vibe coding with Claude Code", summary="A coding agent edits a repo.")
    axe = _video(title="Primitive Technology: Stone Axe", summary="Tool use in the wild.")
    assert affinity(coding, "agentic-engineering") >= 2
    assert affinity(axe, "agentic-engineering") == 0


def test_llm_category_is_ai_engineering_and_comedy_is_not() -> None:
    llm = _video(
        title="Let's build GPT",
        category_ids=("llms-generative-ai",),
        summary="A language model, spelled out.",
    )
    joke = _video(title="Monty Python", content_type="comedy", category_ids=("llms-generative-ai",))
    assert affinity(llm, "ai-engineering") == 3
    assert affinity(joke, "ai-engineering") == 0


def test_safety_category_is_research() -> None:
    safety = _video(title="Alignment", category_ids=("ai-safety-impact",), summary="AI safety.")
    assert affinity(safety, "ai-research") == 3
