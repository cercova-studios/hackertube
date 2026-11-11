"""Argument parser. Unknown flags exit 2. No-args prints the live home view."""

from __future__ import annotations

import argparse
import re
import sys
from typing import Any, Never

from hackertube.commands import Flags, run
from hackertube.errors import AppError, UsageError
from hackertube.output import emit, error_document

DESCRIPTION = "Ranked videos and insights for agentic engineering, AI engineering, and AI research."


class Parser(argparse.ArgumentParser):
    def __init__(self, *args: Any, **kwargs: Any) -> None:
        kwargs.setdefault("allow_abbrev", False)
        super().__init__(*args, **kwargs)

    def error(self, message: str) -> Never:
        flag = None
        match = re.search(r"(--[A-Za-z0-9][A-Za-z0-9_-]*)", message)
        if match and "unrecognized" in message:
            flag = match.group(1)
        raise UsageError(message, flag=flag)


def main(argv: list[str] | None = None) -> int:
    flags = Flags(as_json="--json" in _argv(argv), full=False, fields=None)
    try:
        args = _parse(argv)
        flags = Flags(
            as_json=bool(getattr(args, "json", False)),
            full=bool(getattr(args, "full", False)),
            fields=_fields(getattr(args, "fields", None)),
        )
        document, code = run(getattr(args, "command", None), args, flags)
    except UsageError as exc:
        emit(
            error_document(
                "unknown_flag" if exc.flag else "usage",
                exc.detail,
                2,
                ["hackertube --help"],
                flag=exc.flag,
            ),
            as_json=flags.as_json,
        )
        return 2
    except AppError as exc:
        emit(
            error_document(
                exc.code,
                exc.detail,
                1,
                ["hackertube --help"],
                **exc.extra,
            ),
            as_json=flags.as_json,
        )
        return 1
    except BrokenPipeError:
        return 0
    except Exception as exc:  # noqa: BLE001 — last-resort structured error, not a traceback
        emit(
            error_document("internal", f"{type(exc).__name__}: {exc}", 1, ["hackertube --help"]),
            as_json=flags.as_json,
        )
        return 1
    emit(document, as_json=flags.as_json)
    return code


def _parse(argv: list[str] | None) -> argparse.Namespace:
    common = Parser(add_help=False)
    common.add_argument(
        "--json", action="store_true", default=argparse.SUPPRESS, help="JSON instead of TOON"
    )
    common.add_argument(
        "--full", action="store_true", default=argparse.SUPPRESS, help="do not truncate text"
    )
    common.add_argument("--fields", default=argparse.SUPPRESS, help="comma-separated columns")

    parser = Parser(prog="hackertube", description=DESCRIPTION, parents=[common])
    sub = parser.add_subparsers(dest="command", parser_class=Parser)

    def add(name: str, help_text: str) -> Parser:
        return sub.add_parser(name, parents=[common], help=help_text, description=help_text)

    ingest = add("ingest", "Load the HackerTube dataset and AI Engineer talks")
    ingest.add_argument("--path", help="dataset zip/dir, or an aietalks snapshot dir")
    ingest.add_argument("--source", choices=("hackertube", "aietalks", "all"), default="all")
    ingest.add_argument("--limit", type=int, help="AI Engineer talks to ingest; 0 means all")
    ingest.add_argument("--refresh", action="store_true", help="re-download cached sources")

    search = add("search", "Semantic plus keyword search")
    search.add_argument("query")
    search.add_argument("--area", choices=("agentic-engineering", "ai-engineering", "ai-research"))
    search.add_argument("--in", dest="in_", choices=("catalog", "transcripts"), default="catalog")
    search.add_argument("--source", choices=("hackertube", "aietalks"))
    search.add_argument("--limit", type=int)

    recommend = add("recommend", "Rank unseen videos in a focus area")
    recommend.add_argument(
        "--area", choices=("agentic-engineering", "ai-engineering", "ai-research")
    )
    recommend.add_argument("--source", choices=("hackertube", "aietalks"))
    recommend.add_argument("--limit", type=int)
    recommend.add_argument("--include-news", action="store_true", dest="include_news")

    similar = add("similar", "Neighbors by embedding, else co-recommendation")
    similar.add_argument("vid")
    similar.add_argument("--limit", type=int)

    show = add("show", "One video card")
    show.add_argument("vid")

    transcript = add("transcript", "AI Engineer markdown, else YouTube captions")
    transcript.add_argument("vid")

    insights = add("insights", "Chapters, claims, mentions, and quotes")
    insights.add_argument("vid")
    insights.add_argument(
        "--llm", action="store_true", help="optional pass; needs HACKERTUBE_LLM_API_KEY"
    )

    for name, help_text in (
        ("seen", "Mark a video seen"),
        ("like", "Record a like; likes stay recommendable"),
        ("dislike", "Hide a video until forget"),
    ):
        mark = add(name, help_text)
        mark.add_argument("vid")

    snooze = add("snooze", "Hide a video until a date")
    snooze.add_argument("vid")
    snooze.add_argument("--days", type=int, help="default 7")
    snooze.add_argument("--until", help="YYYY-MM-DD")

    forget = add("forget", "Remove a seen, like, dislike, or snooze mark")
    forget.add_argument("vid")

    state = add("state", "List saved marks")
    state.add_argument("--limit", type=int)

    add("sources", "Registered sources and row counts")
    add("areas", "Focus areas and eligible counts")
    add("setup", "Install the optional session hook")

    enrich = add("enrich", "Cache YouTube Data API metadata")
    enrich.add_argument("vid", nargs="?")
    enrich.add_argument("--limit", type=int)

    return parser.parse_args(argv)


def _argv(argv: list[str] | None) -> list[str]:
    if argv is None:
        return sys.argv[1:]
    return argv


def _fields(value: object) -> tuple[str, ...] | None:
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        raise UsageError("--fields needs at least one name", flag="--fields")
    return tuple(part.strip() for part in text.split(",") if part.strip())


if __name__ == "__main__":
    sys.exit(main())
