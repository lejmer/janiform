from pathlib import Path


def test_resolve_fit_artifact_paths_appends_run_id_to_explicit_output_directory(
    tmp_path: Path,
) -> None:
    from janiform._workflow_outputs import resolve_fit_artifact_paths

    artifact_paths = resolve_fit_artifact_paths(
        run_id="run-123",
        output_dir=tmp_path / ".janiform",
    )

    assert artifact_paths.run_directory == tmp_path / ".janiform" / "run-123"
    assert artifact_paths.primary_output_path == tmp_path / ".janiform" / "run-123" / "model.json"
    assert artifact_paths.report_path == tmp_path / ".janiform" / "run-123" / "fit_report.json"
    assert artifact_paths.manifest_path == tmp_path / ".janiform" / "run-123" / "run.json"
