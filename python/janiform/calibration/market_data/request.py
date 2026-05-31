from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from math import isfinite
from typing import Any, Optional

from .contracts import (
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationFlatCarryInput,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationKnownNewsInput,
    CalibrationMarketDataColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataPreprocessingSettings,
    CalibrationMarketDataWorkflowScope,
    CalibrationMeanReversionConfiguration,
    CalibrationSessionConfigurationInput,
    CalibrationTickObjectiveDataUsagePolicy,
    validate_known_news_inputs,
    validate_option_surface_scope,
    validate_tick_price_input_policy,
)
from ..diagnostics import CalibrationDiagnosticsRequest
from ..effort import CalibrationEffortOptions, STANDARD_CALIBRATION_EFFORT
from ..errors import calibration_error
from ..staged import CalibrationStage, CalibrationStageSolverSettings
from ...contracts.base import (
    ContractModel,
    parse_json_object,
    require_boolean,
    require_datetime_utc,
    require_mapping,
    require_non_empty_string,
    require_positive_finite_number,
    require_positive_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)
from ...pricing import (
    AutoActivityCompressedPricingIntervalSchedulePolicy,
    AutoExactSparsePricingIntervalSchedulePolicy,
    ExplicitIntervalEndTimestampsUtcPricingIntervalSchedulePolicy,
    PricingIntervalSchedulePolicy,
    default_pricing_interval_schedule_policy,
    pricing_interval_schedule_policy_from_mapping,
)


@dataclass(frozen=True, slots=True)
class MarketDataCalibrationRequest(ContractModel):
    schema_version: str
    run_id: str
    tick_data_source: CalibrationInputFileReference
    session_configuration: CalibrationSessionConfigurationInput
    workflow_scope: CalibrationMarketDataWorkflowScope
    normalization_policy: CalibrationMarketDataNormalizationPolicy
    output_preferences: CalibrationMarketDataOutputPreferences
    column_mapping: CalibrationMarketDataColumnMapping = field(
        default_factory=CalibrationMarketDataColumnMapping
    )
    coarse_data_source: Optional[CalibrationInputFileReference] = None
    optimizer_settings_by_stage: tuple[CalibrationStageSolverSettings, ...] = ()
    preprocessing_settings: Optional[CalibrationMarketDataPreprocessingSettings] = None
    option_surface_source: Optional[CalibrationInputFileReference] = None
    option_surface_valuation_time_utc: Optional[datetime] = None
    carry: Optional[CalibrationFlatCarryInput] = None
    pricing_interval_schedule_policy: PricingIntervalSchedulePolicy = field(
        default_factory=default_pricing_interval_schedule_policy
    )
    dynamics_search_restart_candidate_budget: Optional[int] = None
    calibration_effort: CalibrationEffortOptions = STANDARD_CALIBRATION_EFFORT
    treat_calibration_warnings_as_errors: bool = False
    joint_refinement_option_refinement_candidate_count_override: Optional[int] = None
    joint_refinement_option_refinement_wall_clock_budget_millis: Optional[int] = None
    joint_refinement_audit_option_pricing_enabled: bool = False
    diagnostics: CalibrationDiagnosticsRequest = field(
        default_factory=CalibrationDiagnosticsRequest
    )
    stylized_fact_huber_cutoff: Optional[float] = None
    stylized_fact_admissibility_z_threshold: Optional[float] = None
    known_news_inputs: Optional[CalibrationKnownNewsInput] = None
    mean_reversion: Optional[CalibrationMeanReversionConfiguration] = None
    tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy = (
        CalibrationTickObjectiveDataUsagePolicy.SHORT_HORIZON_WINDOW
    )

    def __post_init__(self) -> None:
        if self.schema_version != CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION:
            raise calibration_error(
                "schema_version",
                f"must be `{CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION}`",
            )

        if not self.run_id.strip():
            raise calibration_error("run_id", "must be a non-empty string")

        if self.tick_data_source.file_format not in (
            CalibrationInputFileFormat.CSV,
            CalibrationInputFileFormat.PARQUET,
        ):
            raise calibration_error("tick_data_source.file_format", "must be one of: csv, parquet")

        if (
            self.option_surface_source is not None
            and self.option_surface_source.file_format
            not in (
                CalibrationInputFileFormat.CSV,
                CalibrationInputFileFormat.PARQUET,
            )
        ):
            raise calibration_error("option_surface_source.file_format", "must be one of: csv, parquet")

        if (
            self.coarse_data_source is not None
            and self.coarse_data_source.file_format
            not in (
                CalibrationInputFileFormat.CSV,
                CalibrationInputFileFormat.PARQUET,
            )
        ):
            raise calibration_error("coarse_data_source.file_format", "must be one of: csv, parquet")

        if self.coarse_data_source is None and self.column_mapping.coarse_data is not None:
            raise calibration_error(
                "column_mapping.coarse_data",
                "must not be provided when coarse_data_source is omitted",
            )

        validate_tick_price_input_policy(
            workflow_scope=self.workflow_scope,
            normalization_policy=self.normalization_policy,
        )

        if (
            self.joint_refinement_audit_option_pricing_enabled
            and self.option_surface_source is None
        ):
            raise calibration_error(
                "joint_refinement_audit_option_pricing_enabled",
                "requires option_surface_source",
            )

        if not isinstance(self.diagnostics, CalibrationDiagnosticsRequest):
            raise calibration_error(
                "diagnostics",
                "must be a CalibrationDiagnosticsRequest",
            )

        if self.diagnostics.option_surface_shape_requested() and self.option_surface_source is None:
            raise calibration_error(
                "diagnostics.extended",
                "option_surface_shape requires option_surface_source",
            )

        validate_option_surface_scope(
            workflow_scope=self.workflow_scope,
            option_surface_source=self.option_surface_source,
            option_surface_column_mapping=self.column_mapping.option_surface,
            option_quote_input_policy=self.normalization_policy.option_quote_input_policy,
            option_surface_valuation_time_utc=self.option_surface_valuation_time_utc,
            carry=self.carry,
        )

        validate_known_news_inputs(
            known_news_inputs=self.known_news_inputs,
            known_news_column_mapping=self.column_mapping.known_news,
        )

        if not isinstance(
            self.pricing_interval_schedule_policy,
            (
                AutoActivityCompressedPricingIntervalSchedulePolicy,
                AutoExactSparsePricingIntervalSchedulePolicy,
                ExplicitIntervalEndTimestampsUtcPricingIntervalSchedulePolicy,
            ),
        ):
            raise calibration_error(
                "pricing_interval_schedule_policy",
                "must be a pricing interval schedule policy contract",
            )

        if (
            self.workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY
            and self.mean_reversion is not None
            and self.mean_reversion.enabled
        ):
            raise calibration_error(
                "mean_reversion",
                "must be omitted when workflow_scope is seasonality_only",
            )

        if (
            self.dynamics_search_restart_candidate_budget is not None
            and self.dynamics_search_restart_candidate_budget <= 0
        ):
            raise calibration_error(
                "dynamics_search_restart_candidate_budget",
                "must be greater than zero when provided",
            )

        if not isinstance(self.calibration_effort, CalibrationEffortOptions):
            raise calibration_error(
                "calibration_effort",
                "must be a CalibrationEffortOptions instance",
            )

        if (
            self.joint_refinement_option_refinement_candidate_count_override is not None
            and self.joint_refinement_option_refinement_candidate_count_override <= 0
        ):
            raise calibration_error(
                "joint_refinement_option_refinement_candidate_count_override",
                "must be greater than zero when provided",
            )

        if (
            self.joint_refinement_option_refinement_wall_clock_budget_millis is not None
            and self.joint_refinement_option_refinement_wall_clock_budget_millis <= 0
        ):
            raise calibration_error(
                "joint_refinement_option_refinement_wall_clock_budget_millis",
                "must be greater than zero when provided",
            )

        if (
            self.stylized_fact_huber_cutoff is not None
            and (
                not isfinite(self.stylized_fact_huber_cutoff)
                or self.stylized_fact_huber_cutoff <= 0.0
            )
        ):
            raise calibration_error(
                "stylized_fact_huber_cutoff",
                "must be finite and greater than zero when provided",
            )

        if (
            self.stylized_fact_admissibility_z_threshold is not None
            and (
                not isfinite(self.stylized_fact_admissibility_z_threshold)
                or self.stylized_fact_admissibility_z_threshold <= 0.0
            )
        ):
            raise calibration_error(
                "stylized_fact_admissibility_z_threshold",
                "must be finite and greater than zero when provided",
            )

        match self.workflow_scope:
            case CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
                selected_stages = (CalibrationStage.SEASONALITY,)
            case CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE:
                selected_stages = (
                    CalibrationStage.SEASONALITY,
                    CalibrationStage.HAWKES,
                    CalibrationStage.RETURN_COMPONENTS,
                    CalibrationStage.LONG_VARIANCE,
                    CalibrationStage.NEWS_SHOCKS,
                    CalibrationStage.RETURN_BACKFIT,
                    CalibrationStage.MEAN_REVERSION,
                    CalibrationStage.JOINT_REFINEMENT,
                    CalibrationStage.OPTION_REFINEMENT,
                    CalibrationStage.FINALIZATION,
                )
            case CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE:
                selected_stages = (
                    CalibrationStage.SEASONALITY,
                    CalibrationStage.HAWKES,
                    CalibrationStage.RETURN_COMPONENTS,
                    CalibrationStage.LONG_VARIANCE,
                    CalibrationStage.NEWS_SHOCKS,
                    CalibrationStage.RETURN_BACKFIT,
                    CalibrationStage.MEAN_REVERSION,
                    CalibrationStage.JOINT_REFINEMENT,
                    CalibrationStage.FINALIZATION,
                )

        selected_stage_set = set(selected_stages)
        configured_stage_set: set[CalibrationStage] = set()

        for stage_settings in self.optimizer_settings_by_stage:
            if stage_settings.stage in configured_stage_set:
                raise calibration_error(
                    "optimizer_settings_by_stage",
                    f"contains duplicate stage '{stage_settings.stage.value}'",
                )

            if stage_settings.stage not in selected_stage_set:
                raise calibration_error(
                    "optimizer_settings_by_stage",
                    (
                        f"contains stage '{stage_settings.stage.value}' outside workflow_scope "
                        f"'{self.workflow_scope.value}'"
                    ),
                )

            configured_stage_set.add(stage_settings.stage)

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> MarketDataCalibrationRequest:
        if not isinstance(raw_payload, dict):
            raise calibration_error("market_data_calibration_request", "must be an object")

        return MarketDataCalibrationRequest(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                calibration_error,
            ),
            run_id=require_non_empty_string(raw_payload, "run_id", "run_id", calibration_error),
            tick_data_source=CalibrationInputFileReference.from_mapping(
                require_mapping(raw_payload, "tick_data_source", "tick_data_source", calibration_error),
                field_path="tick_data_source",
            ),
            coarse_data_source=(
                CalibrationInputFileReference.from_mapping(
                    require_mapping(
                        raw_payload,
                        "coarse_data_source",
                        "coarse_data_source",
                        calibration_error,
                    ),
                    field_path="coarse_data_source",
                )
                if "coarse_data_source" in raw_payload
                and raw_payload["coarse_data_source"] is not None
                else None
            ),
            session_configuration=CalibrationSessionConfigurationInput.from_mapping(
                require_mapping(raw_payload, "session_configuration", "session_configuration", calibration_error),
                field_path="session_configuration",
            ),
            column_mapping=(
                CalibrationMarketDataColumnMapping.from_mapping(
                    require_mapping(
                        raw_payload,
                        "column_mapping",
                        "column_mapping",
                        calibration_error,
                    ),
                    field_path="column_mapping",
                )
                if "column_mapping" in raw_payload and raw_payload["column_mapping"] is not None
                else CalibrationMarketDataColumnMapping()
            ),
            workflow_scope=CalibrationMarketDataWorkflowScope(
                require_string(raw_payload, "workflow_scope", "workflow_scope", calibration_error)
            ),
            normalization_policy=CalibrationMarketDataNormalizationPolicy.from_mapping(
                require_mapping(raw_payload, "normalization_policy", "normalization_policy", calibration_error),
                field_path="normalization_policy",
            ),
            output_preferences=CalibrationMarketDataOutputPreferences.from_mapping(
                require_mapping(raw_payload, "output_preferences", "output_preferences", calibration_error),
                field_path="output_preferences",
            ),
            optimizer_settings_by_stage=tuple(
                CalibrationStageSolverSettings.from_mapping(
                    raw_stage_settings,
                    field_path=f"optimizer_settings_by_stage[{stage_index}]",
                )
                for stage_index, raw_stage_settings in enumerate(
                    require_sequence(
                        raw_payload,
                        "optimizer_settings_by_stage",
                        "optimizer_settings_by_stage",
                        calibration_error,
                    )
                    if "optimizer_settings_by_stage" in raw_payload
                    else ()
                )
            ),
            preprocessing_settings=(
                CalibrationMarketDataPreprocessingSettings.from_mapping(
                    require_mapping(
                        raw_payload,
                        "preprocessing_settings",
                        "preprocessing_settings",
                        calibration_error,
                    ),
                    field_path="preprocessing_settings",
                )
                if "preprocessing_settings" in raw_payload
                and raw_payload["preprocessing_settings"] is not None
                else None
            ),
            option_surface_source=(
                CalibrationInputFileReference.from_mapping(
                    require_mapping(
                        raw_payload,
                        "option_surface_source",
                        "option_surface_source",
                        calibration_error,
                    ),
                    field_path="option_surface_source",
                )
                if "option_surface_source" in raw_payload and raw_payload["option_surface_source"] is not None
                else None
            ),
            option_surface_valuation_time_utc=(
                require_datetime_utc(
                    raw_payload["option_surface_valuation_time_utc"],
                    "option_surface_valuation_time_utc",
                    calibration_error,
                )
                if "option_surface_valuation_time_utc" in raw_payload
                and raw_payload["option_surface_valuation_time_utc"] is not None
                else None
            ),
            carry=(
                CalibrationFlatCarryInput.from_mapping(
                    require_mapping(raw_payload, "carry", "carry", calibration_error),
                    field_path="carry",
                )
                if "carry" in raw_payload and raw_payload["carry"] is not None
                else None
            ),
            pricing_interval_schedule_policy=(
                pricing_interval_schedule_policy_from_mapping(
                    require_mapping(
                        raw_payload,
                        "pricing_interval_schedule_policy",
                        "pricing_interval_schedule_policy",
                        calibration_error,
                    ),
                    field_path="pricing_interval_schedule_policy",
                )
                if "pricing_interval_schedule_policy" in raw_payload
                and raw_payload["pricing_interval_schedule_policy"] is not None
                else default_pricing_interval_schedule_policy()
            ),
            dynamics_search_restart_candidate_budget=(
                require_positive_integer(
                    raw_payload,
                    "dynamics_search_restart_candidate_budget",
                    "dynamics_search_restart_candidate_budget",
                    calibration_error,
                )
                if "dynamics_search_restart_candidate_budget" in raw_payload
                and raw_payload["dynamics_search_restart_candidate_budget"] is not None
                else None
            ),
            calibration_effort=(
                CalibrationEffortOptions.from_mapping(
                    require_mapping(
                        raw_payload,
                        "calibration_effort",
                        "calibration_effort",
                        calibration_error,
                    ),
                    field_path="calibration_effort",
                )
                if "calibration_effort" in raw_payload
                and raw_payload["calibration_effort"] is not None
                else STANDARD_CALIBRATION_EFFORT
            ),
            treat_calibration_warnings_as_errors=(
                require_boolean(
                    raw_payload,
                    "treat_calibration_warnings_as_errors",
                    "treat_calibration_warnings_as_errors",
                    calibration_error,
                )
                if "treat_calibration_warnings_as_errors" in raw_payload
                else False
            ),
            joint_refinement_option_refinement_candidate_count_override=(
                require_positive_integer(
                    raw_payload,
                    "joint_refinement_option_refinement_candidate_count_override",
                    "joint_refinement_option_refinement_candidate_count_override",
                    calibration_error,
                )
                if "joint_refinement_option_refinement_candidate_count_override" in raw_payload
                and raw_payload["joint_refinement_option_refinement_candidate_count_override"] is not None
                else None
            ),
            joint_refinement_option_refinement_wall_clock_budget_millis=(
                require_positive_integer(
                    raw_payload,
                    "joint_refinement_option_refinement_wall_clock_budget_millis",
                    "joint_refinement_option_refinement_wall_clock_budget_millis",
                    calibration_error,
                )
                if "joint_refinement_option_refinement_wall_clock_budget_millis" in raw_payload
                and raw_payload["joint_refinement_option_refinement_wall_clock_budget_millis"] is not None
                else None
            ),
            joint_refinement_audit_option_pricing_enabled=(
                require_boolean(
                    raw_payload,
                    "joint_refinement_audit_option_pricing_enabled",
                    "joint_refinement_audit_option_pricing_enabled",
                    calibration_error,
                )
                if "joint_refinement_audit_option_pricing_enabled" in raw_payload
                else False
            ),
            diagnostics=(
                CalibrationDiagnosticsRequest.from_mapping(
                    require_mapping(
                        raw_payload,
                        "diagnostics",
                        "diagnostics",
                        calibration_error,
                    ),
                    field_path="diagnostics",
                )
                if "diagnostics" in raw_payload
                and raw_payload["diagnostics"] is not None
                else CalibrationDiagnosticsRequest()
            ),
            stylized_fact_huber_cutoff=(
                require_positive_finite_number(
                    raw_payload,
                    "stylized_fact_huber_cutoff",
                    "stylized_fact_huber_cutoff",
                    calibration_error,
                )
                if "stylized_fact_huber_cutoff" in raw_payload
                and raw_payload["stylized_fact_huber_cutoff"] is not None
                else None
            ),
            stylized_fact_admissibility_z_threshold=(
                require_positive_finite_number(
                    raw_payload,
                    "stylized_fact_admissibility_z_threshold",
                    "stylized_fact_admissibility_z_threshold",
                    calibration_error,
                )
                if "stylized_fact_admissibility_z_threshold" in raw_payload
                and raw_payload["stylized_fact_admissibility_z_threshold"] is not None
                else None
            ),
            known_news_inputs=(
                CalibrationKnownNewsInput.from_mapping(
                    require_mapping(raw_payload, "known_news_inputs", "known_news_inputs", calibration_error),
                    field_path="known_news_inputs",
                )
                if "known_news_inputs" in raw_payload and raw_payload["known_news_inputs"] is not None
                else None
            ),
            mean_reversion=(
                CalibrationMeanReversionConfiguration.from_mapping(
                    require_mapping(raw_payload, "mean_reversion", "mean_reversion", calibration_error),
                    field_path="mean_reversion",
                )
                if "mean_reversion" in raw_payload and raw_payload["mean_reversion"] is not None
                else None
            ),
            tick_objective_data_usage_policy=(
                CalibrationTickObjectiveDataUsagePolicy(
                    require_string(
                        raw_payload,
                        "tick_objective_data_usage_policy",
                        "tick_objective_data_usage_policy",
                        calibration_error,
                    )
                )
                if "tick_objective_data_usage_policy" in raw_payload
                and raw_payload["tick_objective_data_usage_policy"] is not None
                else CalibrationTickObjectiveDataUsagePolicy.SHORT_HORIZON_WINDOW
            ),
        )


def validate_market_data_calibration_request(raw_request: dict[str, Any] | Any) -> MarketDataCalibrationRequest:
    return MarketDataCalibrationRequest.from_mapping(raw_request)


def serialize_market_data_calibration_request(request: MarketDataCalibrationRequest) -> str:
    return serialize_deterministic_json(request)


def deserialize_market_data_calibration_request(payload_json: str) -> MarketDataCalibrationRequest:
    raw_payload = parse_json_object(payload_json, "market_data_calibration_request", calibration_error)
    return validate_market_data_calibration_request(dict(raw_payload))


__all__ = [
    "MarketDataCalibrationRequest",
    "deserialize_market_data_calibration_request",
    "serialize_market_data_calibration_request",
    "validate_market_data_calibration_request",
]
