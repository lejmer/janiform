from pathlib import Path

import pytest


@pytest.fixture(scope="session")
def project_root() -> Path:
    return Path(__file__).resolve().parents[2]


@pytest.fixture(scope="session")
def python_source_root(project_root: Path) -> Path:
    return project_root / "python"
