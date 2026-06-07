import json
from pathlib import Path

import pytest

from janiform.calibration import CalibrationValidationError
from janiform.pricing import VanillaOptionPricingValidationError
from janiform.simulation import SeededTickSimulationValidationError
from tests.support.workflow_response_contracts import (
    BOUNDARY_RESPONSE_SCENARIOS,
    FAILURE_SCENARIOS,
    RESPONSE_SCENARIOS,
    FailureScenario,
    ResponseScenario,
    build_schema_profile,
    load_workflow_response_snapshot_fixture,
    scenario_failure_message,
    scenario_label,
)


@pytest.mark.parametrize("scenario", RESPONSE_SCENARIOS, ids=scenario_label)
def test_workflow_response_contract_matrix_matches_snapshot(
    scenario: ResponseScenario,
    calendar_registry,
    tmp_path: Path,
) -> None:
    snapshot_fixture = load_workflow_response_snapshot_fixture()
    observed_payload = scenario.builder(calendar_registry, tmp_path)
    observed_profile = build_schema_profile(observed_payload)
    expected_profile = snapshot_fixture["scenarios"][scenario.fixture_key]

    assert observed_profile == expected_profile, scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=(
            "response schema profile drift detected.\n"
            f"expected={json.dumps(expected_profile, sort_keys=True)}\n"
            f"observed={json.dumps(observed_profile, sort_keys=True)}"
        ),
    )


@pytest.mark.parametrize("scenario", BOUNDARY_RESPONSE_SCENARIOS, ids=scenario_label)
def test_workflow_boundary_response_profiles_are_stable_across_repeated_runs(
    scenario: ResponseScenario,
    calendar_registry,
    tmp_path: Path,
) -> None:
    first_profile = build_schema_profile(scenario.builder(calendar_registry, tmp_path))
    second_profile = build_schema_profile(scenario.builder(calendar_registry, tmp_path))

    assert first_profile == second_profile, scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=(
            "boundary response profile changed between repeated runs.\n"
            f"first={json.dumps(first_profile, sort_keys=True)}\n"
            f"second={json.dumps(second_profile, sort_keys=True)}"
        ),
    )


@pytest.mark.parametrize("scenario", FAILURE_SCENARIOS, ids=scenario_label)
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

    assert failure.error_id == scenario.expected_error_id, scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=f"expected error_id={scenario.expected_error_id} observed={failure.error_id}",
    )
    assert failure.field_path == scenario.expected_field_path, scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=f"expected field_path={scenario.expected_field_path} observed={failure.field_path}",
    )
    assert scenario.expected_message_substring in failure.message, scenario_failure_message(
        contract_id=scenario.contract_id,
        scenario_id=scenario.scenario_id,
        detail=(
            f"expected message substring={scenario.expected_message_substring!r} "
            f"observed={failure.message!r}"
        ),
    )
