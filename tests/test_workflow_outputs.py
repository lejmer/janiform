from pathlib import Path

from janiform.pricing import run_vanilla_option_pricing

from tests.support.pricing import build_anchored_runtime_execution_input_model


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


def test_build_pricing_report_payload_preserves_spectral_pricing_diagnostics(tmp_path: Path) -> None:
    from janiform._workflow_outputs import build_pricing_report_payload

    pricing_result = run_vanilla_option_pricing(build_anchored_runtime_execution_input_model())

    report_payload = build_pricing_report_payload(
        output_path=tmp_path / "pricing.json",
        pricing_result=pricing_result,
    )

    assert report_payload["workflow"] == "price"
    assert report_payload["run_diagnostics"] is not None
    assert report_payload["run_diagnostics"]["spectral"] is not None
