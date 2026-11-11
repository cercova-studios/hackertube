"""Agent-owned feedback. The file is the source of truth; the catalog is not."""

from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import date
from pathlib import Path

from hackertube.paths import now_iso

STATUSES = ("seen", "liked", "disliked", "snoozed")


@dataclass(frozen=True)
class Mark:
    status: str
    at: str
    until: str


def path_for(home: Path) -> Path:
    return home / "state.json"


def load(home: Path) -> dict[str, Mark]:
    file = path_for(home)
    if not file.exists():
        return {}
    payload = json.loads(file.read_text(encoding="utf-8"))
    items = payload.get("items", {})
    marks: dict[str, Mark] = {}
    if not isinstance(items, dict):
        return marks
    for vid, raw in items.items():
        if not isinstance(raw, dict):
            continue
        status = str(raw.get("status") or "")
        if status not in STATUSES:
            continue
        marks[str(vid)] = Mark(
            status=status,
            at=str(raw.get("at") or ""),
            until=str(raw.get("until") or ""),
        )
    return marks


def save(home: Path, marks: dict[str, Mark]) -> None:
    home.mkdir(parents=True, exist_ok=True)
    items = {
        vid: {"status": mark.status, "at": mark.at, "until": mark.until or None}
        for vid, mark in sorted(marks.items())
    }
    document = {"version": 1, "items": items}
    path = path_for(home)
    path.write_text(json.dumps(document, indent=2, sort_keys=False) + "\n", encoding="utf-8")


def set_mark(home: Path, vid: str, status: str, *, until: str = "") -> bool:
    """Return True when the stored mark changed. Repeating the same mark is a no-op."""
    marks = load(home)
    current = marks.get(vid)
    if current is not None and current.status == status and current.until == until:
        return False
    marks[vid] = Mark(status=status, at=now_iso(), until=until)
    save(home, marks)
    return True


def forget(home: Path, vid: str) -> bool:
    marks = load(home)
    if vid not in marks:
        return False
    del marks[vid]
    save(home, marks)
    return True


def blocks(mark: Mark | None, day: date) -> bool:
    """Seen, disliked, and unexpired snoozes leave the recommendation list. Likes do not."""
    if mark is None:
        return False
    if mark.status in {"seen", "disliked"}:
        return True
    if mark.status == "snoozed":
        if not mark.until:
            return True
        return date.fromisoformat(mark.until) >= day
    return False


def counts(marks: dict[str, Mark], day: date) -> dict[str, int]:
    tallies = {status: 0 for status in STATUSES}
    for mark in marks.values():
        if mark.status == "snoozed" and not blocks(mark, day):
            continue
        tallies[mark.status] += 1
    return tallies
