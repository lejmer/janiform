import json
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

import annotationlib
import pytest

from tests.public_api_contract_guardrails import (
    PUBLIC_API_CONTRACT_APPROVAL_POLICY,
    PUBLIC_API_CONTRACT_GUARDRAIL_MANIFEST_SCHEMA_VERSION,
    PUBLIC_API_CONTRACT_SNAPSHOT_SCHEMA_VERSION,
    PUBLIC_API_SURFACE_DOCUMENT_VERSION,
    _dataclass_contract_model_signature,
    build_public_api_contract_snapshot,
    compute_public_api_contract_snapshot_sha256,
    public_api_contract_snapshot_matches_approved,
    render_public_api_contract_snapshot_diff,
    validate_public_api_contract_guardrail_manifest,
)

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
PUBLIC_API_CONTRACT_FIXTURE_ROOT = REPOSITORY_ROOT / "tests" / "fixtures" / "python_api_contracts"
PUBLIC_API_CONTRACT_SNAPSHOT_PATH = (
    PUBLIC_API_CONTRACT_FIXTURE_ROOT / "public_api_contract_snapshot.json"
)
PUBLIC_API_CONTRACT_GUARDRAIL_MANIFEST_PATH = (
    PUBLIC_API_CONTRACT_FIXTURE_ROOT / "public_api_contract_guardrails.json"
)


def test_public_api_contract_snapshot_matches_checked_in_fixture() -> None:
    approved_snapshot = _load_json_document(PUBLIC_API_CONTRACT_SNAPSHOT_PATH)
    current_snapshot = build_public_api_contract_snapshot()

    assert public_api_contract_snapshot_matches_approved(current_snapshot, approved_snapshot), (
        "Public API contract drift detected. "
        "Update the public contract docs, regenerate the snapshot fixture, and record the rationale "
        "in tests/fixtures/python_api_contracts/public_api_contract_guardrails.json.\n\n"
        + render_public_api_contract_snapshot_diff(approved_snapshot, current_snapshot)
    )


def test_public_api_contract_guardrail_manifest_approves_checked_in_snapshot() -> None:
    approved_snapshot = _load_json_document(PUBLIC_API_CONTRACT_SNAPSHOT_PATH)
    guardrail_manifest = _load_json_document(PUBLIC_API_CONTRACT_GUARDRAIL_MANIFEST_PATH)

    validate_public_api_contract_guardrail_manifest(guardrail_manifest, approved_snapshot)


def test_public_api_contract_guardrail_rejects_undocumented_snapshot_drift() -> None:
    approved_snapshot = _minimal_snapshot(module_exports=["ExistingSymbol"])
    current_snapshot = _minimal_snapshot(module_exports=["ExistingSymbol", "NewSymbol"])
    guardrail_manifest = _guardrail_manifest_for_snapshot(approved_snapshot)

    diff = render_public_api_contract_snapshot_diff(approved_snapshot, current_snapshot)

    assert not public_api_contract_snapshot_matches_approved(current_snapshot, approved_snapshot)
    assert "NewSymbol" in diff

    validate_public_api_contract_guardrail_manifest(guardrail_manifest, approved_snapshot)


def test_public_api_contract_guardrail_accepts_documented_snapshot_update() -> None:
    approved_snapshot = _minimal_snapshot(module_exports=["ExistingSymbol", "NewSymbol"])
    guardrail_manifest = _guardrail_manifest_for_snapshot(approved_snapshot)

    assert public_api_contract_snapshot_matches_approved(approved_snapshot, approved_snapshot)

    validate_public_api_contract_guardrail_manifest(guardrail_manifest, approved_snapshot)


def test_public_api_contract_snapshot_tracks_supported_cli_modules() -> None:
    current_snapshot = build_public_api_contract_snapshot()

    assert {
        "janiform.cli",
        "janiform.cli.commands.calibration",
        "janiform.cli.commands.simulation",
    } <= set(current_snapshot["public_modules"])

    cli_exports = current_snapshot["public_modules"]["janiform.cli"]["exports"]
    assert "CliExitCode" in cli_exports
    assert "_build_parser" not in cli_exports


def test_public_api_contract_guardrail_supports_public_domain_submodules() -> None:
    approved_snapshot = _minimal_snapshot(
        public_modules={
            "janiform.contracts": [
                "MeanReversionAnchorModel",
                "ContractModel",
                "MeanReversionFittedParameters",
            ],
            "janiform.simulation": ["SeededTickSimulationRequest", "run_seeded_tick_event_simulation"],
            "janiform.simulation.coarse": [
                "CoarseSimulationRunRequest",
                "run_coarse_simulation",
            ],
            "janiform.simulation.seeded": [
                "SeededTickSimulationRequest",
                "run_seeded_tick_event_simulation",
            ],
            "janiform.simulation.shared": [
                "HawkesKernelParameters",
                "SeededTickSimulationValidationError",
            ],
            "janiform.calibration": [
                "JaniformEstimator",
                "MarketDataCalibrationRequest",
            ],
            "janiform.calibration.fit": [
                "JaniformEstimator",
                "JaniformModel",
            ],
            "janiform.calibration.fit.estimator": [
                "JaniformEstimator",
            ],
            "janiform.calibration.fit.model": [
                "JaniformModel",
            ],
            "janiform.calibration.fit.report": [
                "JaniformFitReport",
            ],
            "janiform.calibration.market_data": [
                "MarketDataCalibrationRequest",
                "prepare_market_data_calibration",
            ],
            "janiform.calibration.market_data.contracts": [
                "CalibrationInputFileReference",
                "CalibrationMarketDataWorkflowScope",
            ],
            "janiform.calibration.market_data.request": [
                "MarketDataCalibrationRequest",
                "validate_market_data_calibration_request",
            ],
            "janiform.calibration.market_data.ingestion": [
                "LoadedMarketDataSourceSummary",
            ],
            "janiform.calibration.market_data.translation": [
                "MarketDataTranslationResult",
                "translate_market_data_request_to_prepared_calibration",
            ],
            "janiform.calibration.market_data.workflow": [
                "PreparedMarketDataCalibration",
                "prepare_market_data_calibration",
            ],
            "janiform.calibration.presets": [
                "PresetRegistry",
                "resolve_preset_workflow_configuration",
            ],
            "janiform.calibration.presets.references": [
                "MarketProfileReference",
                "SessionPresetReference",
            ],
            "janiform.calibration.presets.registry": [
                "PresetRegistry",
                "serialize_preset_artifact",
            ],
            "janiform.calibration.presets.resolution": [
                "ResolvedPresetWorkflowConfiguration",
                "resolve_preset_workflow_configuration",
            ],
            "janiform.calibration.sessions": [
                "MarketDataSessionResolution",
            ],
            "janiform.calibration.sessions.contracts": [
                "MarketDataSessionRegularWindow",
                "MarketDataSessionResolution",
            ],
            "janiform.calibration.staged": [
                "StagedCalibrationExecutionInput",
                "run_staged_calibration",
            ],
            "janiform.calibration.staged.contracts": [
                "CalibrationExecutionMode",
                "CalibrationStage",
            ],
            "janiform.calibration.staged.inputs": [
                "StagedCalibrationExecutionInput",
                "validate_staged_calibration_execution_input",
            ],
            "janiform.calibration.staged.request": [
                "StagedCalibrationRequest",
                "validate_staged_calibration_request",
            ],
            "janiform.calibration.staged.results": [
                "StagedCalibrationResponse",
                "validate_staged_calibration_response",
            ],
            "janiform.calibration.staged.runtime": [
                "run_staged_calibration",
            ],
            "janiform.cli": [
                "CliExitCode",
                "CLI_REPOSITORY_RELATIVE_PATH_POLICY",
            ],
            "janiform.cli.commands.calibration": [],
            "janiform.cli.commands.simulation": [
                "run_coarse_simulation",
                "run_seeded_tick_event_simulation",
            ],
        }
    )
    guardrail_manifest = _guardrail_manifest_for_snapshot(approved_snapshot)

    assert set(approved_snapshot["public_modules"]) == {
        "janiform.contracts",
        "janiform.calibration",
        "janiform.calibration.fit",
        "janiform.calibration.fit.estimator",
        "janiform.calibration.fit.model",
        "janiform.calibration.fit.report",
        "janiform.calibration.market_data",
        "janiform.calibration.market_data.contracts",
        "janiform.calibration.market_data.ingestion",
        "janiform.calibration.market_data.request",
        "janiform.calibration.market_data.translation",
        "janiform.calibration.market_data.workflow",
        "janiform.calibration.presets",
        "janiform.calibration.presets.references",
        "janiform.calibration.presets.registry",
        "janiform.calibration.presets.resolution",
        "janiform.calibration.sessions",
        "janiform.calibration.sessions.contracts",
        "janiform.calibration.staged",
        "janiform.calibration.staged.contracts",
        "janiform.calibration.staged.inputs",
        "janiform.calibration.staged.request",
        "janiform.calibration.staged.results",
        "janiform.calibration.staged.runtime",
        "janiform.cli",
        "janiform.cli.commands.calibration",
        "janiform.cli.commands.simulation",
        "janiform.simulation",
        "janiform.simulation.coarse",
        "janiform.simulation.seeded",
        "janiform.simulation.shared",
    }

    validate_public_api_contract_guardrail_manifest(guardrail_manifest, approved_snapshot)


def test_dataclass_contract_model_signature_falls_back_for_annotation_lookup_value_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    @dataclass
    class ExampleContract:
        value: int

    def raise_value_error(*args: Any, **kwargs: Any) -> dict[str, str]:
        raise ValueError("annotation formatting unavailable")

    monkeypatch.setattr(annotationlib, "get_annotations", raise_value_error)

    assert _dataclass_contract_model_signature(ExampleContract) == "(value: 'int') -> 'None'"


def test_dataclass_contract_model_signature_does_not_swallow_unexpected_runtime_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    @dataclass
    class ExampleContract:
        value: int

    def raise_runtime_error(*args: Any, **kwargs: Any) -> dict[str, str]:
        raise RuntimeError("unexpected annotation failure")

    monkeypatch.setattr(annotationlib, "get_annotations", raise_runtime_error)

    with pytest.raises(RuntimeError, match="unexpected annotation failure"):
        _dataclass_contract_model_signature(ExampleContract)


def _load_json_document(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise TypeError(f"{path} must contain a JSON object")

    return payload


def _minimal_snapshot(
    *,
    module_exports: Optional[list[str]] = None,
    public_modules: Optional[Mapping[str, list[str]]] = None,
) -> dict[str, Any]:
    module_export_map: Mapping[str, list[str]]
    if public_modules is not None:
        module_export_map = public_modules
    elif module_exports is not None:
        module_export_map = {"janiform.simulation": module_exports}
    else:
        raise TypeError("module_exports or public_modules must be provided")

    return {
        "schema_version": PUBLIC_API_CONTRACT_SNAPSHOT_SCHEMA_VERSION,
        "public_api_surface_document_version": PUBLIC_API_SURFACE_DOCUMENT_VERSION,
        "package_root": {
            "module_name": "janiform",
            "exports": ["__version__"],
        },
        "public_modules": {
            module_name: _minimal_module_snapshot(module_name=module_name, module_exports=exports)
            for module_name, exports in module_export_map.items()
        },
    }


def _minimal_module_snapshot(*, module_name: str, module_exports: list[str]) -> dict[str, Any]:
    return {
        "module_name": module_name,
        "exports": module_exports,
        "schema_identifiers": [
            {
                "name": "SIMULATION_EXAMPLE_SCHEMA_VERSION",
                "value": "simulation.example.v1",
                "public_exported": False,
            }
        ],
        "symbols": {
            export_name: {
                "kind": "function",
                "signature": "() -> None",
            }
            for export_name in module_exports
        },
    }


def _guardrail_manifest_for_snapshot(snapshot: Mapping[str, Any]) -> dict[str, Any]:
    snapshot_sha256 = compute_public_api_contract_snapshot_sha256(snapshot)

    return {
        "schema_version": PUBLIC_API_CONTRACT_GUARDRAIL_MANIFEST_SCHEMA_VERSION,
        "public_api_surface_document_version": PUBLIC_API_SURFACE_DOCUMENT_VERSION,
        "approval_policy": PUBLIC_API_CONTRACT_APPROVAL_POLICY,
        "approved_snapshot_sha256": snapshot_sha256,
        "change_log": [
            {
                "snapshot_sha256": snapshot_sha256,
                "approved_on": "2026-03-07",
                "summary": "Synthetic approval for test coverage.",
                "rationale": "Synthetic test manifest proving that approved snapshot updates are accepted.",
                "docs_updated": [
                    "docs/development/guides/python_api_contract_compatibility_and_version_guardrails.md"
                ],
            }
        ],
    }
