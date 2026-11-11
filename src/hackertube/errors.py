"""Structured failures. Commands catch these and render them on stdout."""

from __future__ import annotations


class AppError(Exception):
    """Operational failure. Exit 1. The catalog, network, or video was unusable."""

    def __init__(self, code: str, detail: str, **extra: object) -> None:
        super().__init__(detail)
        self.code = code
        self.detail = detail
        self.exit_code = 1
        self.extra = extra


class UsageError(Exception):
    """Bad invocation. Exit 2. Unknown flags, invalid choices, missing arguments."""

    def __init__(self, detail: str, *, flag: str | None = None) -> None:
        super().__init__(detail)
        self.detail = detail
        self.flag = flag
        self.exit_code = 2
