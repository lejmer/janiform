from datetime import UTC, datetime
import re
from typing import Any, cast

import pytest

from janiform.calendar import CalendarRegistry
from janiform.hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlyNewsIntensityProviderMaterializedSource,
    HourlySeasonalityMaterialization,
    HourlySeasonalityMaterializationSettings,
    HourlySeasonalityProviderExplicitSource,
    HourlySeasonalityProviderMaterializedSource,
    materialize_hourly_seasonality_factors,
    provide_hourly_factor_contract,
)
from janiform.seasonality import SeasonalityInputValidationError
from tests.support.fixture_validation import (
    HourlyFactorRegressionCanonicalOutputCase,
    HourlyFactorRegressionFixtureDocument,
    HourlyFactorRegressionMissingDataGuardCase,
    STATE_CLASSIFICATION_STRICTNESS_POLICY,
)

type HourlyCanonicalOutputCase = HourlyFactorRegressionCanonicalOutputCase
type HourlyMissingDataGuardCase = HourlyFactorRegressionMissingDataGuardCase


def test_hourly_factor_regression_fixture_covers_required_case_tags(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
    hourly_factor_regression_missing_data_guard_cases: list[HourlyMissingDataGuardCase],
) -> None:
    canonical_metadata: dict[str, Any] = cast(
        dict[str, Any],
        hourly_factor_regression_fixture_document["canonical_regression_metadata"],
    )
    required_case_tags: set[str] = set(cast(list[str], canonical_metadata["required_tags"]))
    observed_tags: set[str] = {
        cast(str, tag)
        for regression_case in hourly_factor_regression_canonical_cases
        for tag in cast(list[str], regression_case["tags"])
    }
    observed_tags.update(
        cast(str, tag)
        for regression_case in hourly_factor_regression_missing_data_guard_cases
        for tag in cast(list[str], regression_case["tags"])
    )

    assert required_case_tags <= observed_tags


def test_hourly_factor_regression_fixture_identifiers_remain_stable(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
    hourly_factor_regression_missing_data_guard_cases: list[HourlyMissingDataGuardCase],
) -> None:
    canonical_metadata: dict[str, Any] = cast(
        dict[str, Any],
        hourly_factor_regression_fixture_document["canonical_regression_metadata"],
    )
    observed_canonical_fixture_ids: set[str] = {
        _fixture_id(regression_case) for regression_case in hourly_factor_regression_canonical_cases
    }
    observed_missing_data_guard_fixture_ids: set[str] = {
        _fixture_id(regression_case)
        for regression_case in hourly_factor_regression_missing_data_guard_cases
    }

    expected_canonical_fixture_ids: set[str] = set(
        cast(list[str], canonical_metadata["stable_canonical_output_fixture_ids"]),
    )
    expected_missing_data_guard_fixture_ids: set[str] = set(
        cast(list[str], canonical_metadata["stable_missing_data_guard_fixture_ids"]),
    )

    assert observed_canonical_fixture_ids == expected_canonical_fixture_ids
    assert observed_missing_data_guard_fixture_ids == expected_missing_data_guard_fixture_ids


def test_hourly_factor_canonical_output_cases_match_golden_matrix(
    calendar_registry: CalendarRegistry,
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    tolerance_config: dict[str, float] = _tolerance_config(hourly_factor_regression_fixture_document)
    seasonality_absolute_tolerance: float = tolerance_config["s_h_absolute_tolerance"]
    news_intensity_absolute_tolerance: float = tolerance_config["nu_h_absolute_tolerance"]

    for regression_case in hourly_factor_regression_canonical_cases:
        fixture_id: str = _fixture_id(regression_case)
        contract = _build_contract_for_canonical_case(calendar_registry, regression_case)

        _assert_float_series_close(
            observed_values=contract.s_h,
            expected_values=cast(list[float], regression_case["expected_s_h"]),
            absolute_tolerance=seasonality_absolute_tolerance,
            fixture_id=fixture_id,
            field_name="s_h",
        )
        _assert_float_series_close(
            observed_values=contract.nu_h,
            expected_values=cast(list[float], regression_case["expected_nu_h"]),
            absolute_tolerance=news_intensity_absolute_tolerance,
            fixture_id=fixture_id,
            field_name="nu_h",
        )

        assert contract.skipped_local_hour_interval_count == cast(
            int,
            regression_case["expected_skipped_local_hour_interval_count"],
        )
        assert contract.repeated_local_hour_interval_count == cast(
            int,
            regression_case["expected_repeated_local_hour_interval_count"],
        )


def test_hourly_factor_missing_data_guard_cases_match_defined_outcomes(
    calendar_registry: CalendarRegistry,
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_missing_data_guard_cases: list[HourlyMissingDataGuardCase],
) -> None:
    tolerance_config: dict[str, float] = _tolerance_config(hourly_factor_regression_fixture_document)
    seasonality_absolute_tolerance: float = tolerance_config["s_h_absolute_tolerance"]

    for regression_case in hourly_factor_regression_missing_data_guard_cases:
        fixture_id: str = _fixture_id(regression_case)
        expected_outcome: dict[str, Any] = cast(dict[str, Any], regression_case["expected_outcome"])
        outcome_kind: str = cast(str, expected_outcome["outcome_kind"])

        match outcome_kind:
            case "success":
                first_materialization = _materialize_missing_data_guard_case(
                    calendar_registry=calendar_registry,
                    regression_case=regression_case,
                )
                second_materialization = _materialize_missing_data_guard_case(
                    calendar_registry=calendar_registry,
                    regression_case=regression_case,
                )
                expected_s_h: list[float] = cast(list[float], expected_outcome["expected_s_h"])

                _assert_float_series_close(
                    observed_values=first_materialization.s_h,
                    expected_values=expected_s_h,
                    absolute_tolerance=seasonality_absolute_tolerance,
                    fixture_id=fixture_id,
                    field_name="s_h",
                )
                assert first_materialization.sparse_interval_count == cast(
                    int,
                    expected_outcome["expected_sparse_interval_count"],
                )
                assert first_materialization.interpolated_interval_count == cast(
                    int,
                    expected_outcome["expected_interpolated_interval_count"],
                )
                assert first_materialization.fallback_interval_count == cast(
                    int,
                    expected_outcome["expected_fallback_interval_count"],
                )
                assert _seasonality_materialization_snapshot(
                    first_materialization,
                ) == _seasonality_materialization_snapshot(second_materialization)
            case "failure":
                expected_error_message_substring: str = cast(
                    str,
                    expected_outcome["expected_error_message_substring"],
                )
                with pytest.raises(
                    SeasonalityInputValidationError,
                    match=re.escape(expected_error_message_substring),
                ):
                    _materialize_missing_data_guard_case(
                        calendar_registry=calendar_registry,
                        regression_case=regression_case,
                    )
            case _:
                raise ValueError(f"unexpected expected_outcome.outcome_kind value: {outcome_kind}")


def test_hourly_factor_deterministic_fallback_cases_are_repeatable(
    calendar_registry: CalendarRegistry,
    hourly_factor_regression_missing_data_guard_cases: list[HourlyMissingDataGuardCase],
) -> None:
    deterministic_fallback_cases: list[HourlyMissingDataGuardCase] = [
        regression_case
        for regression_case in hourly_factor_regression_missing_data_guard_cases
        if _case_has_tag(regression_case, "deterministic_fallback")
    ]

    assert deterministic_fallback_cases

    for regression_case in deterministic_fallback_cases:
        expected_outcome: dict[str, Any] = cast(dict[str, Any], regression_case["expected_outcome"])
        if cast(str, expected_outcome["outcome_kind"]) == "success":
            first_materialization = _materialize_missing_data_guard_case(
                calendar_registry=calendar_registry,
                regression_case=regression_case,
            )
            second_materialization = _materialize_missing_data_guard_case(
                calendar_registry=calendar_registry,
                regression_case=regression_case,
            )

            assert _seasonality_materialization_snapshot(
                first_materialization,
            ) == _seasonality_materialization_snapshot(second_materialization)


def test_hourly_factor_threshold_policy_applies_factor_ranges_to_canonical_cases(
    calendar_registry: CalendarRegistry,
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    threshold_configuration = _threshold_configuration(hourly_factor_regression_fixture_document)
    assert (
        cast(str, threshold_configuration["state_classification_strictness"])
        == STATE_CLASSIFICATION_STRICTNESS_POLICY
    )

    for regression_case in hourly_factor_regression_canonical_cases:
        fixture_id: str = _fixture_id(regression_case)
        contract = _build_contract_for_canonical_case(calendar_registry, regression_case)
        range_policy = _factor_range_policy_for_case(
            threshold_configuration=threshold_configuration,
            regression_case=regression_case,
        )

        _assert_values_within_range(
            observed_values=contract.s_h,
            minimum=cast(float, range_policy["s_h_min"]),
            maximum=cast(float, range_policy["s_h_max"]),
            fixture_id=fixture_id,
            field_name="s_h",
        )
        _assert_values_within_range(
            observed_values=contract.nu_h,
            minimum=cast(float, range_policy["nu_h_min"]),
            maximum=cast(float, range_policy["nu_h_max"]),
            fixture_id=fixture_id,
            field_name="nu_h",
        )


def test_hourly_factor_threshold_policy_range_boundaries_are_inclusive() -> None:
    _assert_values_within_range(
        observed_values=[0.9, 1.2],
        minimum=0.9,
        maximum=1.2,
        fixture_id="boundary_fixture",
        field_name="s_h",
    )


def test_hourly_factor_threshold_policy_range_check_fails_when_exceeded() -> None:
    with pytest.raises(AssertionError, match="outside allowed range"):
        _assert_values_within_range(
            observed_values=[1.2000000001],
            minimum=0.9,
            maximum=1.2,
            fixture_id="boundary_fixture",
            field_name="s_h",
        )


def _build_contract_for_canonical_case(
    calendar_registry: CalendarRegistry,
    regression_case: HourlyCanonicalOutputCase,
):
    seasonality_source = _parse_seasonality_provider_source(
        cast(dict[str, Any], regression_case["seasonality_source"]),
    )
    news_intensity_source = _parse_news_intensity_provider_source(
        cast(dict[str, Any], regression_case["news_intensity_source"]),
    )
    range_start_utc = _parse_fixture_timestamp(cast(str, regression_case["range_start_utc"]))
    range_end_utc = _parse_fixture_timestamp(cast(str, regression_case["range_end_utc"]))

    return provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=cast(str, regression_case["exchange_code"]),
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        seasonality_source=seasonality_source,
        news_intensity_source=news_intensity_source,
    )


def _parse_seasonality_provider_source(
    raw_source: dict[str, Any],
) -> HourlySeasonalityProviderExplicitSource | HourlySeasonalityProviderMaterializedSource:
    source_kind: str = cast(str, raw_source["source_kind"])

    match source_kind:
        case "explicit":
            return HourlySeasonalityProviderExplicitSource(
                hourly_factors=cast(list[float], raw_source["hourly_factors"]),
                materialization_method=cast(str, raw_source["materialization_method"]),
            )
        case "materialized":
            source_timestamps_utc: list[datetime] = [
                _parse_fixture_timestamp(raw_timestamp)
                for raw_timestamp in cast(list[str], raw_source["source_timestamps_utc"])
            ]
            return HourlySeasonalityProviderMaterializedSource(
                source_timestamps_utc=source_timestamps_utc,
                source_seasonality_factors=cast(list[float], raw_source["source_seasonality_factors"]),
                interpolation_method=cast(str, raw_source["interpolation_method"]),
                aggregation_method=cast(str, raw_source["aggregation_method"]),
                sparse_fallback_policy=cast(str, raw_source["sparse_fallback_policy"]),
                sparse_hour_minimum_observation_count=cast(
                    int,
                    raw_source["sparse_hour_minimum_observation_count"],
                ),
            )
        case _:
            raise ValueError(f"unexpected seasonality source_kind value: {source_kind}")


def _parse_news_intensity_provider_source(
    raw_source: dict[str, Any],
) -> HourlyNewsIntensityProviderExplicitSource | HourlyNewsIntensityProviderMaterializedSource:
    source_kind: str = cast(str, raw_source["source_kind"])

    match source_kind:
        case "explicit":
            return HourlyNewsIntensityProviderExplicitSource(
                hourly_factors=cast(list[float], raw_source["hourly_factors"]),
                materialization_method=cast(str, raw_source["materialization_method"]),
            )
        case "materialized":
            known_scheduled_event_timestamps_utc: list[datetime] = [
                _parse_fixture_timestamp(raw_known_timestamp)
                for raw_known_timestamp in cast(
                    list[str],
                    raw_source["known_scheduled_event_timestamps_utc"],
                )
            ]
            raw_surprise_multipliers = cast(
                dict[str, float],
                raw_source["surprise_deterministic_time_multiplier_by_interval_index"],
            )
            surprise_deterministic_time_multiplier_by_interval_index: dict[int, float] = {
                int(raw_interval_index): float(raw_multiplier_value)
                for raw_interval_index, raw_multiplier_value in raw_surprise_multipliers.items()
            }

            return HourlyNewsIntensityProviderMaterializedSource(
                known_scheduled_event_timestamps_utc=known_scheduled_event_timestamps_utc,
                surprise_open_baseline_intensity=float(raw_source["surprise_open_baseline_intensity"]),
                surprise_closed_baseline_intensity=float(raw_source["surprise_closed_baseline_intensity"]),
                surprise_deterministic_time_multiplier_by_interval_index=(
                    surprise_deterministic_time_multiplier_by_interval_index
                ),
            )
        case _:
            raise ValueError(f"unexpected news_intensity source_kind value: {source_kind}")


def _materialize_missing_data_guard_case(
    *,
    calendar_registry: CalendarRegistry,
    regression_case: HourlyMissingDataGuardCase,
) -> HourlySeasonalityMaterialization:
    range_start_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["range_start_utc"]))
    range_end_utc: datetime = _parse_fixture_timestamp(cast(str, regression_case["range_end_utc"]))
    settings = cast(dict[str, Any], regression_case["settings"])
    source_timestamps_utc: list[datetime] = [
        _parse_fixture_timestamp(raw_timestamp)
        for raw_timestamp in cast(list[str], regression_case["source_timestamps_utc"])
    ]

    return materialize_hourly_seasonality_factors(
        calendar_registry=calendar_registry,
        exchange_code=cast(str, regression_case["exchange_code"]),
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        source_timestamps_utc=source_timestamps_utc,
        source_seasonality_factors=cast(list[float], regression_case["source_seasonality_factors"]),
        settings=HourlySeasonalityMaterializationSettings(
            interpolation_method=cast(str, settings["interpolation_method"]),
            aggregation_method=cast(str, settings["aggregation_method"]),
            sparse_fallback_policy=cast(str, settings["sparse_fallback_policy"]),
            sparse_hour_minimum_observation_count=cast(
                int,
                settings["sparse_hour_minimum_observation_count"],
            ),
        ),
    )


def _seasonality_materialization_snapshot(
    materialization: HourlySeasonalityMaterialization,
) -> dict[str, Any]:
    return {
        "s_h": list(materialization.s_h),
        "sparse_interval_count": materialization.sparse_interval_count,
        "interpolated_interval_count": materialization.interpolated_interval_count,
        "fallback_interval_count": materialization.fallback_interval_count,
        "skipped_local_hour_interval_count": materialization.skipped_local_hour_interval_count,
        "repeated_local_hour_interval_count": materialization.repeated_local_hour_interval_count,
    }


def _assert_float_series_close(
    *,
    observed_values: list[float],
    expected_values: list[float],
    absolute_tolerance: float,
    fixture_id: str,
    field_name: str,
) -> None:
    assert len(observed_values) == len(expected_values), (
        f"fixture {fixture_id} {field_name} length mismatch "
        f"{len(observed_values)} != {len(expected_values)}"
    )
    for value_index, observed_value in enumerate(observed_values):
        expected_value = expected_values[value_index]
        absolute_difference = abs(observed_value - expected_value)
        assert absolute_difference <= absolute_tolerance, (
            f"fixture {fixture_id} {field_name}[{value_index}] expected {expected_value} "
            f"observed {observed_value} with abs_diff {absolute_difference}"
        )


def _assert_values_within_range(
    *,
    observed_values: list[float],
    minimum: float,
    maximum: float,
    fixture_id: str,
    field_name: str,
) -> None:
    for value_index, observed_value in enumerate(observed_values):
        assert minimum <= observed_value <= maximum, (
            f"fixture {fixture_id} {field_name}[{value_index}] value {observed_value} "
            f"is outside allowed range [{minimum}, {maximum}]"
        )


def _case_has_tag(regression_case: HourlyMissingDataGuardCase, tag: str) -> bool:
    return tag in cast(list[str], regression_case["tags"])


def _fixture_id(regression_case: HourlyCanonicalOutputCase | HourlyMissingDataGuardCase) -> str:
    return cast(str, regression_case["fixture_id"])


def _parse_fixture_timestamp(timestamp_utc: str) -> datetime:
    return datetime.fromisoformat(timestamp_utc.replace("Z", "+00:00")).astimezone(UTC)


def _threshold_configuration(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> dict[str, Any]:
    return cast(
        dict[str, Any],
        hourly_factor_regression_fixture_document["threshold_configuration"],
    )


def _tolerance_config(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> dict[str, float]:
    threshold_configuration = _threshold_configuration(hourly_factor_regression_fixture_document)
    return cast(dict[str, float], threshold_configuration["tolerance_config"])


def _factor_range_policy_for_case(
    *,
    threshold_configuration: dict[str, Any],
    regression_case: HourlyCanonicalOutputCase,
) -> dict[str, float]:
    factor_range_by_fixture_class: dict[str, dict[str, float]] = cast(
        dict[str, dict[str, float]],
        threshold_configuration["factor_range_by_fixture_class"],
    )
    matched_fixture_classes: list[str] = sorted(
        fixture_class
        for fixture_class in factor_range_by_fixture_class
        if fixture_class in cast(list[str], regression_case["tags"])
    )

    assert len(matched_fixture_classes) == 1, (
        f"fixture {_fixture_id(regression_case)} must match exactly one threshold fixture class, "
        f"matched={matched_fixture_classes}"
    )

    return factor_range_by_fixture_class[matched_fixture_classes[0]]
