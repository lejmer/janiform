from __future__ import annotations

from copy import deepcopy
from typing import Any, Mapping, Optional, cast

from ....contracts import serialize_pretty_json
from ....mean_reversion import MeanReversionFittedParameters
from ...errors import calibration_error
from ...market_data.contracts import CalibrationMarketDataWorkflowScope
from ...market_data.workflow import PreparedMarketDataCalibration
from ...presets.resolution import (
    DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME,
    ResolvedPresetSelection,
    ResolvedPresetWorkflowConfiguration,
    ResolvedWorkflowComponentSource,
)
from ...staged.contracts import CalibrationStage
from ...staged.results import StagedCalibrationResponse
from .shared import MODEL_PERSISTENCE_SCHEMA_VERSION


def build_model_persistence_payload(model: Any) -> dict[str, Any]:
    return {
        "schema_version": MODEL_PERSISTENCE_SCHEMA_VERSION,
        "run_id": model.run_id,
        "workflow_scope": model.workflow_scope.value,
        "parameters": deepcopy(model.parameters),
        "metadata": deepcopy(model.metadata),
        "resolved_configuration": model.resolved_configuration.to_payload(),
        "session_resolution": model.session_resolution.to_payload(),
    }


def extract_model_parameters(stage_report_payloads: tuple[dict[str, Any], ...]) -> dict[str, Any]:
    parameters: dict[str, Any] = {}
    for stage_report in stage_report_payloads:
        stage_kind = _require_stage_report_kind(stage_report)
        match stage_kind:
            case CalibrationStage.SEASONALITY:
                normalized_stage_payload = stage_report.get("normalized_stage_payload")
                if isinstance(normalized_stage_payload, Mapping):
                    parameters["seasonality"] = {
                        "schema_version": normalized_stage_payload.get("composition_schema_version"),
                        "exchange_code": normalized_stage_payload.get("exchange_code"),
                        "intraday_minutes_per_bin": normalized_stage_payload.get("intraday_minutes_per_bin"),
                        "intraday_factor_by_bin": tuple(
                            cast(list[float], normalized_stage_payload.get("intraday_factor_by_bin", []))
                        ),
                        "day_of_week_factor": tuple(
                            cast(list[float], normalized_stage_payload.get("day_of_week_factor", []))
                        ),
                        "day_of_year_factor": tuple(
                            cast(list[float], normalized_stage_payload.get("day_of_year_factor", []))
                        ),
                    }
            case CalibrationStage.HAWKES:
                fitted_stage_payload = stage_report.get("fitted_stage_payload")
                if isinstance(fitted_stage_payload, Mapping):
                    parameters["hawkes"] = {
                        "schema_version": fitted_stage_payload.get("schema_version"),
                        "selected_candidate_id": fitted_stage_payload.get("selected_candidate_id"),
                        "fitted_parameters": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("fitted_parameters", {}))
                        ),
                    }
            case CalibrationStage.RETURN_COMPONENTS:
                fitted_stage_payload = stage_report.get("fitted_stage_payload")
                if isinstance(fitted_stage_payload, Mapping):
                    parameters["return_components"] = {
                        "schema_version": fitted_stage_payload.get("schema_version"),
                        "selected_candidate_id": fitted_stage_payload.get("selected_candidate_id"),
                        "fitted_parameters": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("fitted_parameters", {}))
                        ),
                    }
            case CalibrationStage.MEAN_REVERSION:
                fitted_stage_payload = stage_report.get("fitted_stage_payload")
                if isinstance(fitted_stage_payload, Mapping):
                    parameters["mean_reversion"] = {
                        "schema_version": fitted_stage_payload.get("schema_version"),
                        "fitted_parameters": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("fitted_parameters", {}))
                        ),
                        "diagnostics": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("diagnostics", {}))
                        ),
                    }
            case CalibrationStage.JOINT_REFINEMENT:
                fitted_stage_payload = stage_report.get("fitted_stage_payload")
                if isinstance(fitted_stage_payload, Mapping):
                    parameters["joint_refinement"] = {
                        "schema_version": fitted_stage_payload.get("schema_version"),
                        "selected_candidate_id": fitted_stage_payload.get("selected_candidate_id"),
                        "fitted_parameters": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("fitted_parameters", {}))
                        ),
                        "baseline_hawkes_fitted_parameters": deepcopy(
                            cast(
                                dict[str, Any],
                                fitted_stage_payload.get("baseline_hawkes_fitted_parameters", {}),
                            )
                        ),
                        "baseline_return_components_fitted_parameters": deepcopy(
                            cast(
                                dict[str, Any],
                                fitted_stage_payload.get("baseline_return_components_fitted_parameters", {}),
                            )
                        ),
                        "objective_breakdown": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("objective_breakdown", {}))
                        ),
                        "optimizer_diagnostics": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("optimizer_diagnostics", {}))
                        ),
                        "option_calibration_diagnostics": deepcopy(
                            cast(
                                dict[str, Any],
                                fitted_stage_payload.get("option_calibration_diagnostics", {}),
                            )
                        ),
                    }
    return parameters


def build_model_metadata(
    *,
    resolved_configuration: ResolvedPresetWorkflowConfiguration,
    prepared: PreparedMarketDataCalibration,
    staged_response: StagedCalibrationResponse,
    stage_report_payloads: tuple[dict[str, Any], ...],
    input_inference: dict[str, Any],
    reference_price: Optional[float],
) -> dict[str, Any]:
    market_profile_name = (
        resolved_configuration.market_profile.selected_preset.preset_name
        if (
            resolved_configuration.market_profile is not None
            and resolved_configuration.market_profile.selected_preset is not None
        )
        else None
    )

    session_preset_name = (
        resolved_configuration.session.selected_preset.preset_name
        if resolved_configuration.session.selected_preset is not None
        else None
    )

    known_news_preset_name = (
        resolved_configuration.known_news.selected_preset.preset_name
        if resolved_configuration.known_news.selected_preset is not None
        else None
    )

    known_news_event_count = len(resolved_configuration.known_news.configuration.events)

    calibration_scope = (
        "joint_option_calibrated"
        if prepared.diagnostics.workflow_scope == CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE
        else (
            "seasonality_only"
            if prepared.diagnostics.workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY
            else "underlying_only"
        )
    )

    return {
        "run_id": staged_response.run_id,
        "workflow_scope": prepared.diagnostics.workflow_scope.value,
        "calibration_scope": calibration_scope,
        "execution_mode": staged_response.execution_mode.value,
        "selected_stages": [stage.value for stage in prepared.staged_request.selected_stages],
        "market_profile": market_profile_name,
        "session_preset": session_preset_name,
        "known_news_preset": known_news_preset_name,
        "session_source_kind": resolved_configuration.session.source_kind.value,
        "known_news_source_kind": resolved_configuration.known_news.source_kind.value,
        "fitted_parameter_summary": {
            "stage_keys": sorted(extract_model_parameters(stage_report_payloads).keys()),
            "selected_candidate_ids": selected_candidate_ids(stage_report_payloads),
        },
        "preset_provenance": {
            "market_profile": (
                resolved_component_provenance(
                    resolved_configuration.market_profile.source_kind,
                    resolved_configuration.market_profile.selected_preset,
                    resolved_configuration.market_profile.applied_override_fields,
                )
                if resolved_configuration.market_profile is not None
                else None
            ),
            "session": {
                **resolved_component_provenance(
                    resolved_configuration.session.source_kind,
                    resolved_configuration.session.selected_preset,
                    resolved_configuration.session.applied_override_fields,
                ),
                "inferred_from_data": resolved_configuration.session.inferred_from_data,
            },
            "known_news": {
                **resolved_component_provenance(
                    resolved_configuration.known_news.source_kind,
                    resolved_configuration.known_news.selected_preset,
                    resolved_configuration.known_news.applied_override_fields,
                ),
                "recommended_preset": (
                    resolved_configuration.known_news.recommended_reference.preset_name
                    if resolved_configuration.known_news.recommended_reference is not None
                    else None
                ),
                "recommendation_policy": (
                    resolved_configuration.known_news.recommendation_policy.value
                    if resolved_configuration.known_news.recommendation_policy is not None
                    else None
                ),
                "recommendation_applied": resolved_configuration.known_news.recommendation_applied,
            },
        },
        "session_resolution": {
            "resolution_kind": prepared.session_resolution.resolution_kind,
            "provenance_kind": prepared.session_resolution.provenance_kind,
            "selected_policy_id": prepared.session_resolution.selected_policy_id,
            "exchange_code": prepared.session_resolution.exchange_code,
            "exchange_name": prepared.session_resolution.exchange_name,
            "resolved_timezone_name": prepared.session_resolution.resolved_timezone_name,
            "confidence_score": prepared.session_resolution.confidence_score,
            "confidence_label": prepared.session_resolution.confidence_label,
            "manual_override_applied": prepared.session_resolution.manual_override_applied,
        },
        "known_news_activation": {
            "selected_preset": known_news_preset_name or DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME,
            "event_count": known_news_event_count,
            "active": known_news_event_count > 0,
        },
        "simulation_output_preferences": {
            "reference_price": reference_price,
        },
        "diagnostic_artifacts": {
            "artifact_bundle_available": staged_response.diagnostic_artifact_bundle is not None,
            "artifact_families": diagnostic_artifact_families(staged_response),
        },
        "input_inference": input_inference,
        "diagnostics": [
            {
                "component": diagnostic.component.value,
                "severity": diagnostic.severity.value,
                "code": diagnostic.code,
                "message": diagnostic.message,
            }
            for diagnostic in resolved_configuration.diagnostics
        ],
    }


def diagnostic_artifact_families(staged_response: StagedCalibrationResponse) -> list[str]:
    artifact_families: list[str] = []
    for reference in staged_response.diagnostic_artifact_references:
        artifact_family = reference.get("artifact_family")
        if isinstance(artifact_family, str):
            artifact_families.append(artifact_family)

    return artifact_families


def selected_candidate_ids(stage_report_payloads: tuple[dict[str, Any], ...]) -> dict[str, str]:
    selected_candidate_ids_by_stage: dict[str, str] = {}
    for stage_report in stage_report_payloads:
        raw_stage_kind = stage_report.get("stage_report_kind")
        fitted_stage_payload = stage_report.get("fitted_stage_payload")
        if not isinstance(raw_stage_kind, str) or not isinstance(fitted_stage_payload, Mapping):
            continue

        try:
            stage_kind = CalibrationStage(raw_stage_kind)
        except ValueError:
            continue

        selected_candidate_id = fitted_stage_payload.get("selected_candidate_id")
        if isinstance(selected_candidate_id, str):
            selected_candidate_ids_by_stage[stage_kind.value] = selected_candidate_id

    return selected_candidate_ids_by_stage


def normalize_model_parameters(raw_parameters: Mapping[str, Any]) -> dict[str, Any]:
    parameters = deepcopy(dict(raw_parameters))
    raw_seasonality = parameters.get("seasonality")
    if isinstance(raw_seasonality, dict):
        for field_name in (
            "intraday_factor_by_bin",
            "day_of_week_factor",
            "day_of_year_factor",
        ):
            raw_value = raw_seasonality.get(field_name)
            if isinstance(raw_value, list):
                raw_seasonality[field_name] = tuple(raw_value)

    normalize_mean_reversion_model_parameters(parameters)

    return parameters


def normalize_mean_reversion_model_parameters(parameters: dict[str, Any]) -> None:
    raw_mean_reversion = parameters.get("mean_reversion")
    if isinstance(raw_mean_reversion, Mapping):
        fitted_parameters = raw_mean_reversion.get("fitted_parameters")
        if isinstance(fitted_parameters, Mapping):
            raw_mean_reversion["fitted_parameters"] = MeanReversionFittedParameters.from_mapping(
                dict(fitted_parameters),
                field_path="parameters.mean_reversion.fitted_parameters",
            ).to_payload()

    raw_return_components = parameters.get("return_components")
    if isinstance(raw_return_components, Mapping):
        fitted_parameters = raw_return_components.get("fitted_parameters")
        if isinstance(fitted_parameters, Mapping):
            normalize_return_components_mean_reversion_block(
                fitted_parameters,
                field_path="parameters.return_components.fitted_parameters",
            )

    raw_joint_refinement = parameters.get("joint_refinement")
    if not isinstance(raw_joint_refinement, Mapping):
        return

    raw_fitted_parameters = raw_joint_refinement.get("fitted_parameters")
    if isinstance(raw_fitted_parameters, Mapping):
        raw_return_components_fitted_parameters = raw_fitted_parameters.get(
            "return_components_fitted_parameters"
        )
        if isinstance(raw_return_components_fitted_parameters, Mapping):
            normalize_return_components_mean_reversion_block(
                raw_return_components_fitted_parameters,
                field_path="parameters.joint_refinement.fitted_parameters.return_components_fitted_parameters",
            )

    raw_baseline_return_components = raw_joint_refinement.get(
        "baseline_return_components_fitted_parameters"
    )
    if isinstance(raw_baseline_return_components, Mapping):
        normalize_return_components_mean_reversion_block(
            raw_baseline_return_components,
            field_path="parameters.joint_refinement.baseline_return_components_fitted_parameters",
        )


def normalize_return_components_mean_reversion_block(
    fitted_parameters: Mapping[str, Any],
    *,
    field_path: str,
) -> None:
    if not isinstance(fitted_parameters, dict):
        return

    raw_mean_reversion = fitted_parameters.get("mean_reversion_fitted_parameters")
    if not isinstance(raw_mean_reversion, Mapping):
        raise calibration_error(
            f"{field_path}.mean_reversion_fitted_parameters",
            "must be an object",
        )

    fitted_parameters["mean_reversion_fitted_parameters"] = MeanReversionFittedParameters.from_mapping(
        dict(raw_mean_reversion),
        field_path=f"{field_path}.mean_reversion_fitted_parameters",
    ).to_payload()


def resolved_component_provenance(
    source_kind: ResolvedWorkflowComponentSource,
    selected_preset: Optional[ResolvedPresetSelection],
    applied_override_fields: tuple[str, ...],
) -> dict[str, Any]:
    return {
        "source_kind": source_kind.value,
        "selected_preset": (
            None
            if selected_preset is None
            else {
                "category": selected_preset.category.value,
                "registry_scope": selected_preset.registry_scope.value,
                "preset_name": selected_preset.preset_name,
            }
        ),
        "applied_override_fields": list(applied_override_fields),
    }


def serialize_model_payload(model: Any) -> str:
    return serialize_pretty_json(build_model_persistence_payload(model))


def _require_stage_report_kind(stage_report: Mapping[str, Any]) -> CalibrationStage:
    raw_stage_kind = stage_report.get("stage_report_kind")
    if not isinstance(raw_stage_kind, str):
        raise calibration_error("stage_report_kind", "must be a string")

    try:
        return CalibrationStage(raw_stage_kind)
    except ValueError as error:
        raise calibration_error(
            "stage_report_kind",
            f"unsupported stage report kind `{raw_stage_kind}`",
        ) from error
