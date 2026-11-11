import zipfile
from pathlib import Path

import pytest

from hackertube.errors import AppError
from hackertube.sources.hackertube_dataset import _safe_extract


def test_zip_slip_is_refused(tmp_path: Path) -> None:
    archive_path = tmp_path / "bad.zip"
    with zipfile.ZipFile(archive_path, "w") as archive:
        archive.writestr("../escape.txt", "nope")
    with pytest.raises(AppError) as caught:
        _safe_extract(archive_path, tmp_path / "out")
    assert caught.value.code == "unsafe_zip"
