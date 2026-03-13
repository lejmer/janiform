import json
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import pytest

from janiform.calibration import CalibrationValidationError
from janiform.calibration.staged import (
    CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
    CalibrationExecutionMode,
    run_staged_calibration,
    validate_staged_calibration_execution_input,
    validate_staged_calibration_request,
    validate_staged_calibration_response,
)
from janiform.hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
)
from janiform.pricing import (
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
    VanillaOptionPricingValidationError,
    run_vanilla_option_pricing,
    validate_vanilla_option_pricing_request,
    validate_vanilla_option_pricing_response,
    validate_vanilla_option_pricing_run_result,
)
from janiform.simulation import (
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SeededTickEventSide,
    SeededTickSimulationValidationError,
    SeededTickSimulationRequest,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
    load_hourly_factor_contract_for_simulation,
    run_seeded_tick_event_simulation,
    validate_seeded_tick_simulation_response,
)
from tests.calibration_test_support import (
    build_valid_execution_input_model,
    build_valid_execution_input_payload,
)

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
WORKFLOW_RESPONSE_SCHEMA_SNAPSHOT_PATH = (
    REPOSITORY_ROOT / "tests" / "fixtures" / "python_api_contracts" / "workflow_response_schema_snapshot.json"
)
WORKFLOW_RESPONSE_SCHEMA_SNAPSHOT_VERSION = "python.workflow_response_schema_snapshot.v1"
SEMANTIC_STRING_FIELD_NAMES: frozenset[str] = frozenset(
    {
        "calibration_hooks_schema_version",
        "completed_stage",
        "event_ordering_policy",
        "event_record_schema_version",
        "execution_mode",
        "failure_triage_policy",
        "handoff_policy",
        "hook_policy",
        "run_diagnostics_schema_version",
        "run_result_schema_version",
        "schema_version",
        "side_encoding_policy",
        "stage",
        "stage_report_kind",
        "state_snapshot_capture",
        "state_snapshot_schema_version",
        "status",
        "timestamp_representation_policy",
    }
)
LITERAL_STRING_ARRAY_FIELD_NAMES: frozenset[str] = frozenset(
    {
        "required_calibration_hooks_fields",
        "required_run_diagnostics_fields",
        "required_run_result_fields",
    }
)


@dataclass(frozen=True, slots=True)
class ResponseScenario:
    contract_id: str
    scenario_id: str
    builder: Callable[[Any, Path], dict[str, Any]]
    repeated_boundary_run: bool = False

    @property
    def fixture_key(self) -> str:
        return f"{self.contract_id}::{self.scenario_id}"


@dataclass(frozen=True, slots=True)
class FailureScenario:
    contract_id: str
    scenario_id: str
    runner: Callable[[Any], None]
    expected_error_id: str
    expected_field_path: str
    expected_message_substring: str


def _response_scenarios() -> tuple[ResponseScenario, ...]:
    return (
        ResponseScenario(
            contract_id="simulation.seeded_tick_event.path.v1",
            scenario_id="direct_response_validation_nominal",
            builder=_build_simulation_direct_response_payload,
        ),
        ResponseScenario(
            contract_id="simulation.seeded_tick_event.path.v1",
            scenario_id="boundary_runtime_nominal",
            builder=_build_simulation_boundary_response_payload,
            repeated_boundary_run=True,
        ),
        ResponseScenario(
            contract_id="pricing.vanilla_option.response.contract",
            scenario_id="direct_response_validation_nominal",
            builder=_build_pricing_direct_response_payload,
        ),
        ResponseScenario(
            contract_id="pricing.vanilla_option.run_result.v1",
            scenario_id="direct_run_result_validation_edge_optional_outputs_omitted",
            builder=_build_pricing_direct_run_result_payload,
        ),
        ResponseScenario(
            contract_id="pricing.vanilla_option.run_result.v1",
            scenario_id="boundary_runtime_nominal",
            builder=_build_pricing_boundary_run_result_payload,
            repeated_boundary_run=True,
        ),
        ResponseScenario(
            contract_id=CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
            scenario_id="direct_response_validation_nominal",
            builder=_build_calibration_direct_response_payload,
        ),
        ResponseScenario(
            contract_id=CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
            scenario_id="boundary_runtime_default_memory",
            builder=_build_calibration_boundary_memory_payload,
            repeated_boundary_run=True,
        ),
        ResponseScenario(
            contract_id=CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
            scenario_id="boundary_runtime_explicit_disk_override",
            builder=_build_calibration_boundary_disk_payload,
            repeated_boundary_run=True,
        ),
    )


def _failure_scenarios() -> tuple[FailureScenario, ...]:
    return (
        FailureScenario(
            contract_id="simulation.validation.contract",
            scenario_id="invalid_response_side",
            runner=_run_simulation_invalid_response_side,
            expected_error_id="simulation.validation.contract",
            expected_field_path="seeded_tick_simulation_response.fine_path[0].side",
            expected_message_substring="must be one of: buy, sell",
        ),
        FailureScenario(
            contract_id="pricing.validation.contract",
            scenario_id="invalid_request_missing_strike",
            runner=_run_pricing_invalid_request_missing_strike,
            expected_error_id="pricing.validation.contract",
            expected_field_path="market_inputs.strike",
            expected_message_substring="is required",
        ),
        FailureScenario(
            contract_id="calibration.validation.contract",
            scenario_id="invalid_request_unknown_execution_mode",
            runner=_run_calibration_invalid_request_execution_mode,
            expected_error_id="calibration.validation.contract",
            expected_field_path="execution_mode",
            expected_message_substring="must be one of: memory, disk",
        ),
        FailureScenario(
            contract_id="calibration.validation.contract",
            scenario_id="invalid_execution_input_disk_only_handoff_in_memory_mode",
            runner=_run_calibration_memory_mode_disk_handoff_failure,
            expected_error_id="calibration.validation.contract",
            expected_field_path="stage_handoffs.transport",
            expected_message_substring="requires execution_mode=disk",
        ),
    )


def _build_simulation_direct_response_payload(calendar_registry, tmp_path: Path) -> dict[str, Any]:
    del calendar_registry
    del tmp_path

    return validate_seeded_tick_simulation_response(
        {
            "schema_version": "simulation.seeded_tick_event.path.v1",
            "metadata": {
                "schema_version": "simulation.seeded_tick_event.tape_metadata.v1",
                "path_record_schema_version": "simulation.seeded_tick_event.record.v1",
                "event_projection_record_schema_version": "simulation.seeded_tick_event.record.v1",
                "transition_ordering_policy": "deterministic_tie_break.v1",
                "timestamp_representation_policy": "utc_nanos.v1",
                "side_encoding_policy": "buy_sell.v1",
                "state_snapshot_capture": "disabled",
                "state_snapshot_capture_point_policy": None,
            },
            "event_tape_metadata": {
                "schema_version": "simulation.seeded_tick_event.tape_metadata.v1",
                "event_record_schema_version": "simulation.seeded_tick_event.record.v1",
                "state_snapshot_schema_version": "simulation.seeded_tick_event.state_snapshot.v1",
                "event_ordering_policy": "deterministic_tie_break.v1",
                "timestamp_representation_policy": "utc_nanos.v1",
                "side_encoding_policy": "buy_sell.v1",
                "state_snapshot_capture": "disabled",
                "state_snapshot_capture_point_policy": None,
            },
            "diagnostics": {
                "schema_version": "simulation.seeded_tick.path_diagnostics.v1",
                "transition_count": 1,
                "event_count": 1,
                "native_runtime_seconds": 0.0,
                "python_response_construction_seconds": 0.0,
                "python_materialization_seconds": 0.0,
                "boundary_transport_bytes": 0,
                "native_estimated_retained_bytes": 0,
                "materialized_transition_count": 1,
                "materialized_event_count": 1,
                "boundary_transport_policy": "materialized_payload_projection.v1",
                "boundary_chunk_record_count": 1,
            },
            "fine_path": [
                {
                    "transition_index": 0,
                    "timestamp_utc_nanos": 100,
                    "transition_kind": "buy_trade",
                    "side": SeededTickEventSide.BUY.value,
                    "news_arrival_component": None,
                    "market_state": "open",
                    "return_components": {
                        "buy_side_return": 0.05,
                        "sell_side_return": 0.0,
                        "trading_time_return": 0.05,
                        "open_jump_return": 0.0,
                        "closed_gap_return": 0.0,
                        "total_return": 0.05,
                    },
                    "cumulative_log_return": 0.05,
                }
            ],
            "event_tape": [
                {
                    "event_index": 0,
                    "timestamp_utc_nanos": 100,
                    "side": SeededTickEventSide.BUY.value,
                }
            ],
        }
    ).to_payload()


def _build_simulation_boundary_response_payload(calendar_registry, tmp_path: Path) -> dict[str, Any]:
    del tmp_path

    start_time_utc = datetime(2026, 2, 2, 15, tzinfo=UTC)
    hourly_factor_contract = load_hourly_factor_contract_for_simulation(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=start_time_utc,
        range_end_utc=datetime(2026, 2, 2, 17, tzinfo=UTC),
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=(1.0, 1.15),
            materialization_method="explicit_hourly_input.v1",
        ),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
            hourly_factors=(0.01, 0.02),
            materialization_method="explicit_hourly_input.v1",
        ),
    )
    request = SeededTickSimulationRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=start_time_utc,
        horizon_interval_count=2,
        kernel_parameters=_valid_kernel_parameters(),
        nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
            alpha=2.3,
            beta_0=-0.8,
            delta=0.7,
            mu=0.1,
            delta_beta=-0.2,
        ),
        news_process_configuration=NewsProcessConfiguration(
            schema_version="news.process.configuration.v1",
            arrivals=NewsArrivalConfiguration(
                known=KnownNewsArrivalConfiguration(
                    scheduled_event_timestamps_utc_nanos=(),
                ),
                surprise=SurpriseNewsArrivalConfiguration(
                    baseline_intensity=0.0,
                ),
            ),
            shared_mark_law=SharedNewsMarkLawConfiguration(
                gamma_severity_marks=GammaSeverityMarkConfiguration(
                    shape_k=1.5,
                    scale_theta=0.9,
                ),
                double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                    probability_positive=0.4,
                    eta_plus=1.3,
                    eta_minus=1.1,
                ),
                coupled_gap_marks=CoupledGapMarkConfiguration(sigma_g=0.75),
            ),
            known_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=0.6,
                return_jump_scale=1.2,
            ),
            surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=0.6,
                return_jump_scale=1.2,
            ),
        ),
        initial_excitation_state=_valid_excitation_state(),
        hourly_factor_contract=hourly_factor_contract,
    )

    return run_seeded_tick_event_simulation(request).to_payload()


def _build_pricing_direct_response_payload(calendar_registry, tmp_path: Path) -> dict[str, Any]:
    del calendar_registry
    del tmp_path

    return validate_vanilla_option_pricing_response(
        {
            "price": 12.5,
            "diagnostics": {
                "forward_price": 104.3,
                "discount_factor": 0.97,
                "maturity_interval": 0.5,
                "boundary_state_norm": 0.55,
            },
        }
    ).to_payload()


def _build_pricing_direct_run_result_payload(calendar_registry, tmp_path: Path) -> dict[str, Any]:
    del calendar_registry
    del tmp_path

    return validate_vanilla_option_pricing_run_result(
        {
            "schema_version": "pricing.vanilla_option.run_result.v1",
            "output_schema_metadata": {
                "schema_version": "pricing.vanilla_option.run_output_schema_metadata.v1",
                "run_result_schema_version": "pricing.vanilla_option.run_result.v1",
                "run_diagnostics_schema_version": "pricing.vanilla_option.run_diagnostics.v1",
                "calibration_hooks_schema_version": "pricing.vanilla_option.calibration_hooks.v1",
                "required_run_result_fields": [
                    "schema_version",
                    "output_schema_metadata",
                    "response",
                    "run_diagnostics",
                    "calibration_hooks",
                ],
                "required_run_diagnostics_fields": [
                    "schema_version",
                    "transform",
                    "integration_quality",
                ],
                "required_calibration_hooks_fields": [
                    "schema_version",
                    "hook_policy",
                    "option_price",
                    "log_moneyness",
                    "forward_moneyness",
                    "maturity_interval",
                    "transform_status",
                    "integration_numeric_quality_is_poor",
                    "integration_convergence_check_passed",
                    "integration_truncation_check_passed",
                ],
            },
            "response": {
                "price": 12.5,
                "diagnostics": {
                    "forward_price": 104.3,
                    "discount_factor": 0.97,
                    "maturity_interval": 0.5,
                    "boundary_state_norm": 0.55,
                },
            },
            "run_diagnostics": None,
            "calibration_hooks": None,
        }
    ).to_payload()


def _build_pricing_boundary_run_result_payload(calendar_registry, tmp_path: Path) -> dict[str, Any]:
    del calendar_registry
    del tmp_path

    return run_vanilla_option_pricing(_valid_pricing_execution_input()).to_payload()


def _build_calibration_direct_response_payload(calendar_registry, tmp_path: Path) -> dict[str, Any]:
    del calendar_registry
    del tmp_path

    return validate_staged_calibration_response(
        {
            "schema_version": "calibration.orchestration.staged_run.report.v1",
            "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
            "run_id": "run-001",
            "execution_mode": "memory",
            "status": "success",
            "stage_reports": [
                {
                    "stage_report_kind": "seasonality",
                    "stage": "seasonality",
                    "status": "success",
                    "run_id": "run-001",
                    "execution_mode": "memory",
                    "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                }
            ],
            "emitted_checkpoints": [],
        }
    ).to_payload()


def _build_calibration_boundary_memory_payload(calendar_registry, tmp_path: Path) -> dict[str, Any]:
    del tmp_path

    execution_input = build_valid_execution_input_model()
    return run_staged_calibration(execution_input, calendar_registry=calendar_registry).to_payload()


def _build_calibration_boundary_disk_payload(calendar_registry, tmp_path: Path) -> dict[str, Any]:
    execution_input = build_valid_execution_input_model(execution_mode=CalibrationExecutionMode.DISK)
    execution_input_payload = execution_input.to_payload()

    checkpoint_directory = tmp_path / f"disk_checkpoint_run_{len(list(tmp_path.iterdir()))}"
    checkpoint_directory.mkdir()

    execution_input_payload["runtime_resources"]["disk_checkpoint_directory"] = str(checkpoint_directory)
    validated_execution_input = validate_staged_calibration_execution_input(execution_input_payload)

    return run_staged_calibration(
        validated_execution_input,
        calendar_registry=calendar_registry,
    ).to_payload()


def _run_simulation_invalid_response_side(calendar_registry) -> None:
    del calendar_registry

    validate_seeded_tick_simulation_response(
        {
            "schema_version": "simulation.seeded_tick_event.path.v1",
            "metadata": {
                "schema_version": "simulation.seeded_tick_event.tape_metadata.v1",
                "path_record_schema_version": "simulation.seeded_tick_event.record.v1",
                "event_projection_record_schema_version": "simulation.seeded_tick_event.record.v1",
                "transition_ordering_policy": "deterministic_tie_break.v1",
                "timestamp_representation_policy": "utc_nanos.v1",
                "side_encoding_policy": "buy_sell.v1",
                "state_snapshot_capture": "disabled",
                "state_snapshot_capture_point_policy": None,
            },
            "fine_path": [
                {
                    "transition_index": 0,
                    "timestamp_utc_nanos": 100,
                    "transition_kind": "buy_trade",
                    "side": "hold",
                    "news_arrival_component": None,
                    "market_state": "open",
                    "return_components": {
                        "buy_side_return": 0.05,
                        "sell_side_return": 0.0,
                        "trading_time_return": 0.05,
                        "open_jump_return": 0.0,
                        "closed_gap_return": 0.0,
                        "total_return": 0.05,
                    },
                    "cumulative_log_return": 0.05,
                }
            ],
        }
    )


def _run_pricing_invalid_request_missing_strike(calendar_registry) -> None:
    del calendar_registry

    request_payload = _valid_pricing_execution_input().request.to_payload()
    del request_payload["market_inputs"]["strike"]

    validate_vanilla_option_pricing_request(request_payload)


def _run_calibration_invalid_request_execution_mode(calendar_registry) -> None:
    del calendar_registry

    request_payload = build_valid_execution_input_payload()["request"]
    request_payload["execution_mode"] = "archive"

    validate_staged_calibration_request(request_payload)


def _run_calibration_memory_mode_disk_handoff_failure(calendar_registry) -> None:
    del calendar_registry

    request_payload = build_valid_execution_input_payload()["request"]
    request_payload["stage_handoffs"] = [
        {
            "from_stage": "seasonality",
            "to_stage": "hawkes",
            "transport": "persisted_artifact_reference",
            "artifact_reference": "artifact://seasonality/stage-001",
        }
    ]

    validate_staged_calibration_request(request_payload)


def _valid_kernel_parameters() -> HawkesKernelParameters:
    return HawkesKernelParameters(
        lambda_bar=0.8,
        beta=1.6,
        alpha=HawkesKernelLoadings(
            plus_plus=0.2,
            plus_minus=0.1,
            minus_plus=0.1,
            minus_minus=0.25,
        ),
        rho=HawkesKernelLoadings(
            plus_plus=0.08,
            plus_minus=0.04,
            minus_plus=0.05,
            minus_minus=0.1,
        ),
    )


def _valid_excitation_state() -> HawkesExcitationState:
    return HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )


def _valid_pricing_execution_input() -> VanillaOptionPricingExecutionInput:
    return VanillaOptionPricingExecutionInput(
        request=VanillaOptionPricingRequest(
            market_inputs=VanillaOptionPricingMarketInputs(
                spot=101.0,
                strike=100.0,
                maturity_interval=1.0,
                risk_free_rate=0.02,
                dividend_yield=0.01,
            ),
            payoff=VanillaOptionPayoffConfiguration(payoff_type=VanillaOptionPayoffType.CALL),
            boundary_state=HawkesExcitationState(
                y_plus_plus=0.0,
                y_plus_minus=0.0,
                y_minus_plus=0.0,
                y_minus_minus=0.0,
            ),
            model_parameters=VanillaOptionPricingModelParameters(
                hawkes_kernel_parameters=HawkesKernelParameters(
                    lambda_bar=0.8,
                    beta=1.6,
                    alpha=HawkesKernelLoadings(
                        plus_plus=0.2,
                        plus_minus=0.1,
                        minus_plus=0.1,
                        minus_minus=0.25,
                    ),
                    rho=HawkesKernelLoadings(
                        plus_plus=0.1,
                        plus_minus=0.05,
                        minus_plus=0.05,
                        minus_minus=0.1,
                    ),
                ),
                gamma_severity_mark_configuration=GammaSeverityMarkConfiguration(
                    shape_k=1.8,
                    scale_theta=0.7,
                ),
                nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
                    alpha=2.3,
                    beta_0=-0.8,
                    delta=0.7,
                    mu=0.1,
                    delta_beta=-0.2,
                ),
            ),
        ),
        interval_factors=VanillaOptionPricingIntervalFactors(
            s_h=1.0,
            nu_h_known=0.0,
            nu_h_surprise=0.0,
        ),
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=True,
            include_integration_quality_diagnostics=True,
            include_calibration_hooks=True,
        ),
    )


def _build_schema_profile(value: Any, *, field_name: str | None = None) -> Any:
    if value is None:
        return {"type": "null"}

    if isinstance(value, bool):
        return {"type": "bool"}

    if isinstance(value, int):
        return {"type": "int"}

    if isinstance(value, float):
        return {"type": "float"}

    if isinstance(value, str):
        if field_name in SEMANTIC_STRING_FIELD_NAMES or field_name in LITERAL_STRING_ARRAY_FIELD_NAMES:
            return {"type": "string", "value": value}

        return {"type": "string"}

    if isinstance(value, dict):
        return {
            "type": "object",
            "fields": {
                field_key: _build_schema_profile(field_value, field_name=field_key)
                for field_key, field_value in value.items()
            },
        }

    if isinstance(value, list):
        item_field_name = field_name if field_name in LITERAL_STRING_ARRAY_FIELD_NAMES else None
        unique_item_profiles: list[Any] = []
        seen_profiles: set[str] = set()
        for item in value:
            item_profile = _build_schema_profile(item, field_name=item_field_name)
            serialized_profile = json.dumps(item_profile, sort_keys=True)
            if serialized_profile in seen_profiles:
                continue

            seen_profiles.add(serialized_profile)
            unique_item_profiles.append(item_profile)

        return {
            "type": "array",
            "length": len(value),
            "item_profiles": unique_item_profiles,
        }

    raise TypeError(f"unsupported schema profile value: {type(value)!r}")


def _load_workflow_response_snapshot_fixture() -> dict[str, Any]:
    payload = json.loads(WORKFLOW_RESPONSE_SCHEMA_SNAPSHOT_PATH.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise TypeError("workflow response snapshot fixture must be a JSON object")

    if payload.get("schema_version") != WORKFLOW_RESPONSE_SCHEMA_SNAPSHOT_VERSION:
        raise ValueError(
            "workflow response snapshot fixture schema_version must be "
            f"{WORKFLOW_RESPONSE_SCHEMA_SNAPSHOT_VERSION}"
        )

    return payload


def _scenario_label(scenario: ResponseScenario | FailureScenario) -> str:
    return f"{scenario.contract_id}::{scenario.scenario_id}"


def _scenario_failure_message(*, contract_id: str, scenario_id: str, detail: str) -> str:
    return f"contract_id={contract_id} scenario_id={scenario_id} {detail}"


RESPONSE_SCENARIOS: tuple[ResponseScenario, ...] = _response_scenarios()
BOUNDARY_RESPONSE_SCENARIOS: tuple[ResponseScenario, ...] = tuple(
    scenario
    for scenario in RESPONSE_SCENARIOS
    if scenario.repeated_boundary_run
)
FAILURE_SCENARIOS: tuple[FailureScenario, ...] = _failure_scenarios()


@pytest.mark.parametrize("scenario", RESPONSE_SCENARIOS, ids=_scenario_label)
def test_workflow_response_contract_matrix_matches_snapshot(
    scenario: ResponseScenario,
    calendar_registry,
    tmp_path: Path,
) -> None:
    snapshot_fixture = _load_workflow_response_snapshot_fixture()
    observed_payload = scenario.builder(calendar_registry, tmp_path)
    observed_profile = _build_schema_profile(observed_payload)
    expected_profile = snapshot_fixture["scenarios"][scenario.fixture_key]

    assert observed_profile == expected_profile, _scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=(
            "response schema profile drift detected.\n"
            f"expected={json.dumps(expected_profile, sort_keys=True)}\n"
            f"observed={json.dumps(observed_profile, sort_keys=True)}"
        ),
    )


@pytest.mark.parametrize("scenario", BOUNDARY_RESPONSE_SCENARIOS, ids=_scenario_label)
def test_workflow_boundary_response_profiles_are_stable_across_repeated_runs(
    scenario: ResponseScenario,
    calendar_registry,
    tmp_path: Path,
) -> None:
    first_profile = _build_schema_profile(scenario.builder(calendar_registry, tmp_path))
    second_profile = _build_schema_profile(scenario.builder(calendar_registry, tmp_path))

    assert first_profile == second_profile, _scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=(
            "boundary response profile changed between repeated runs.\n"
            f"first={json.dumps(first_profile, sort_keys=True)}\n"
            f"second={json.dumps(second_profile, sort_keys=True)}"
        ),
    )


@pytest.mark.parametrize("scenario", FAILURE_SCENARIOS, ids=_scenario_label)
def test_workflow_contract_failure_matrix_reports_structured_context(
    scenario: FailureScenario,
    calendar_registry,
) -> None:
    with pytest.raises(
        (
            SeededTickSimulationValidationError,
            VanillaOptionPricingValidationError,
            CalibrationValidationError,
        )
    ) as captured_error:
        scenario.runner(calendar_registry)

    failure = captured_error.value.failure

    assert failure.error_id == scenario.expected_error_id, _scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=f"expected error_id={scenario.expected_error_id} observed={failure.error_id}",
    )
    assert failure.field_path == scenario.expected_field_path, _scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=f"expected field_path={scenario.expected_field_path} observed={failure.field_path}",
    )
    assert scenario.expected_message_substring in failure.message, _scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=(
            f"expected message substring={scenario.expected_message_substring!r} "
            f"observed={failure.message!r}"
        ),
    )
