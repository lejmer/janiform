"""Shared figure output helpers for paper scripts."""

from pathlib import Path
from typing import Any, Optional

import matplotlib.pyplot as plt
from matplotlib.figure import Figure

from figure_metadata import (
    build_figure_metadata,
    file_artifact_metadata,
    write_metadata_json,
)


def write_figure_and_metadata(
    *,
    figure: Figure,
    output_path: Path,
    figure_id: str,
    title: str,
    script_path: Path,
    repository_root: Path,
    inputs: list[dict[str, Any]],
    parameters: dict[str, Any],
    data_summary: dict[str, Any],
    plot_summary: dict[str, Any],
    command: list[str],
    notes: Optional[list[str]] = None,
) -> None:
    print(f"Writing figure to disk: {output_path}")

    output_path.parent.mkdir(parents=True, exist_ok=True)

    figure.tight_layout()
    figure.savefig(output_path, bbox_inches="tight")

    plt.close(figure)

    print(f"Building metadata document: {output_path}")

    metadata = build_figure_metadata(
        figure_id=figure_id,
        title=title,
        script_path=script_path,
        inputs=inputs,
        outputs=[
            file_artifact_metadata(
                output_path,
                role="figure",
                artifact_type="pdf",
                media_type="application/pdf",
                include_sha256=True,
                repository_root=repository_root,
            )
        ],
        parameters=parameters,
        data_summary=data_summary,
        plot_summary=plot_summary,
        command=command,
        repository_root=repository_root,
        notes=notes,
    )

    metadata_path = output_path.with_suffix(output_path.suffix + ".metadata.json")

    print(f"Writing metadata to disk: {metadata_path}")

    write_metadata_json(metadata_path, metadata)
