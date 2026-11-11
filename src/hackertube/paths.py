"""On-disk locations. All mutable state lives under HACKERTUBE_HOME."""

from __future__ import annotations

import os
import shutil
import sys
from datetime import UTC, date, datetime
from pathlib import Path


def home() -> Path:
    override = os.environ.get("HACKERTUBE_HOME")
    if override:
        return Path(override).expanduser()
    return Path.home() / ".hackertube"


def display_path(path: Path) -> str:
    """Render a path with ``~`` so agents can paste it back."""
    try:
        resolved = path.expanduser()
        user_home = Path.home()
        if resolved.is_absolute() and resolved.is_relative_to(user_home):
            suffix = resolved.relative_to(user_home).as_posix()
            return "~" if suffix == "." else f"~/{suffix}"
    except (OSError, ValueError):
        return str(path)
    return str(path)


def bin_path() -> str:
    found = shutil.which("hackertube")
    if found:
        return display_path(Path(found))
    return display_path(Path(sys.argv[0]))


def today() -> date:
    """UTC date. HACKERTUBE_TODAY=YYYY-MM-DD freezes it for tests and replays."""
    override = os.environ.get("HACKERTUBE_TODAY")
    if override:
        return date.fromisoformat(override)
    return datetime.now(UTC).date()


def now_iso() -> str:
    override = os.environ.get("HACKERTUBE_TODAY")
    if override:
        return f"{date.fromisoformat(override).isoformat()}T00:00:00Z"
    current = datetime.now(UTC).replace(microsecond=0)
    return current.strftime("%Y-%m-%dT%H:%M:%SZ")
