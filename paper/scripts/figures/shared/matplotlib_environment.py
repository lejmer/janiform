"""Matplotlib runtime setup shared by paper figure scripts."""

import os
from pathlib import Path


def configure_matplotlib_runtime(repo_root: Path) -> Path:
    matplotlib_config_dir = repo_root / "paper" / "build" / "matplotlib"
    matplotlib_config_dir.mkdir(parents=True, exist_ok=True)

    os.environ.setdefault("MPLCONFIGDIR", str(matplotlib_config_dir))

    return matplotlib_config_dir
