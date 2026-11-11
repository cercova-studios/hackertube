"""Opt-in session hook. Writes under HACKERTUBE_HOME and merges Claude settings."""

from __future__ import annotations

import json
from pathlib import Path

from hackertube.errors import AppError

HOOK_SCRIPT = """#!/bin/sh
# HackerTube session hook. Prints the live home view. Safe to run repeatedly.
exec hackertube
"""

FRAGMENT = {"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "hackertube"}]}]}}


def install(home: Path, claude_settings: Path | None = None) -> dict[str, bool]:
    home.mkdir(parents=True, exist_ok=True)
    hooks = home / "hooks"
    hooks.mkdir(parents=True, exist_ok=True)
    script = hooks / "session.sh"
    script_changed = _write_if_changed(script, HOOK_SCRIPT)
    script.chmod(0o755)
    fragment = hooks / "claude.settings.fragment.json"
    fragment_changed = _write_if_changed(fragment, json.dumps(FRAGMENT, indent=2) + "\n")
    settings_path = claude_settings or (Path.home() / ".claude" / "settings.json")
    settings_changed = _merge_claude(settings_path)
    return {
        "script": script_changed,
        "fragment": fragment_changed,
        "claude_settings": settings_changed,
    }


def _write_if_changed(path: Path, content: str) -> bool:
    if path.exists() and path.read_text(encoding="utf-8") == content:
        return False
    path.write_text(content, encoding="utf-8")
    return True


def _merge_claude(path: Path) -> bool:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        try:
            current = json.loads(path.read_text(encoding="utf-8") or "{}")
        except json.JSONDecodeError as exc:
            raise AppError(
                "settings_invalid",
                f"{path} is not JSON; hook not written",
                path=str(path),
            ) from exc
        if not isinstance(current, dict):
            raise AppError("settings_invalid", f"{path} must be a JSON object", path=str(path))
    else:
        current = {}
    changed = _ensure_hook(current)
    if not changed and path.exists():
        return False
    path.write_text(json.dumps(current, indent=2) + "\n", encoding="utf-8")
    return True


def _ensure_hook(settings: dict[str, object]) -> bool:
    hooks = settings.get("hooks")
    if hooks is None:
        hooks = {}
        settings["hooks"] = hooks
    if not isinstance(hooks, dict):
        raise AppError("settings_invalid", "settings.hooks must be an object")
    session = hooks.get("SessionStart")
    if session is None:
        session = []
        hooks["SessionStart"] = session
    if not isinstance(session, list):
        raise AppError("settings_invalid", "settings.hooks.SessionStart must be a list")
    for entry in session:
        if not isinstance(entry, dict):
            continue
        nested = entry.get("hooks")
        if not isinstance(nested, list):
            continue
        for hook in nested:
            if isinstance(hook, dict) and hook.get("command") == "hackertube":
                return False
    session.append({"hooks": [{"type": "command", "command": "hackertube"}]})
    return True
