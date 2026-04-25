import math
from datetime import UTC, datetime
from typing import Any, Optional, cast

import pytest

from janiform.calendar import CalendarRegistry
from janiform.seasonality import (
    DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    DEFAULT_SEASONALITY_ESTIMATOR_VERSION,
    DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION,
    INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
    SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
    DayOfWeekSeasonalityEstimatorSettings,
    DayOfWeekSeasonalityEstimate,
    DayOfYearSeasonalityEstimatorSettings,
    DayOfYearSeasonalityEstimate,
    DeterministicSeasonalityComposition,
    IntradaySeasonalityEstimate,
    IntradaySeasonalityEstimatorSettings,
    IntradaySmoothingKernel,
    ScheduledEventOverlayDefinition,
    SeasonalityDiagnosticsBatchInput,
    SessionFilter,
    SeasonalityInputValidationError,
    TimestampEventPreprocessingResult,
    compose_deterministic_seasonality_signal,
    estimate_day_of_week_seasonality,
    estimate_day_of_year_seasonality,
    estimate_intraday_seasonality,
    generate_seasonality_diagnostics_report,
    generate_seasonality_diagnostics_reports_batch,
    load_scheduled_event_overlay_definitions,
    preprocess_timestamp_events,
    TimestampEventPreprocessingResult,
)

OPEN_MARKET_STATE_CODE: int = 1
CLOSED_MARKET_STATE_CODE: int = 0


def test_preprocess_timestamp_events_raises_for_missing_timestamp_field(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [{"event_id": "a"}]

    with pytest.raises(
        SeasonalityInputValidationError,
        match="missing required field 'timestamp'",
    ):
        preprocess_timestamp_events(
            event_records,
            exchange_code="XNYS",
            calendar_registry=calendar_registry,
        )


def test_preprocess_timestamp_events_raises_for_invalid_timestamp_string(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [{"timestamp": "not-a-timestamp"}]

    with pytest.raises(
        SeasonalityInputValidationError,
        match="invalid timestamp",
    ):
        preprocess_timestamp_events(
            event_records,
            exchange_code="XNYS",
            calendar_registry=calendar_registry,
        )


def test_preprocess_timestamp_events_normalizes_timezone_and_local_context(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": "2026-02-02T14:30:00Z"},
        {"timestamp": "2026-02-02T09:30:00-05:00"},
    ]

    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )

    expected_timestamp_utc_nanos: int = _utc_nanos(2026, 2, 2, 14, 30, 0)

    assert preprocessed.exchange_timezone_name == "America/New_York"
    assert preprocessed.timestamp_utc_nanos == [
        expected_timestamp_utc_nanos,
        expected_timestamp_utc_nanos,
    ]
    assert preprocessed.exchange_local_minute_of_day == [570, 570]
    assert preprocessed.exchange_local_day_of_week == bytes([1, 1])
    assert preprocessed.market_state_codes == bytes([OPEN_MARKET_STATE_CODE, OPEN_MARKET_STATE_CODE])


def test_preprocess_timestamp_events_filters_by_session_state(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": "2026-02-02T14:30:00Z"},
        {"timestamp": "2026-02-02T03:00:00Z"},
    ]

    open_only = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.OPEN_ONLY,
    )
    closed_only = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.CLOSED_ONLY,
    )

    assert open_only.accepted_record_count == 1
    assert open_only.filtered_record_count == 1
    assert open_only.market_state_codes == bytes([OPEN_MARKET_STATE_CODE])
    assert closed_only.accepted_record_count == 1
    assert closed_only.filtered_record_count == 1
    assert closed_only.market_state_codes == bytes([CLOSED_MARKET_STATE_CODE])


def test_preprocess_timestamp_events_reports_missing_and_sparse_hour_buckets(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": "2026-02-02T14:30:00Z"},
        {"timestamp": "2026-02-02T16:30:00Z"},
        {"timestamp": "2026-02-02T16:31:00Z"},
    ]

    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
        sparse_hour_event_threshold=2,
    )

    assert preprocessed.missing_hour_bucket_utc_nanos == [
        _utc_nanos(2026, 2, 2, 15, 0, 0),
    ]
    assert preprocessed.sparse_hour_bucket_utc_nanos == [
        _utc_nanos(2026, 2, 2, 14, 0, 0),
    ]
    assert preprocessed.missing_hour_bucket_count == 1
    assert preprocessed.sparse_hour_bucket_count == 1


def test_preprocess_timestamp_events_is_deterministic_for_identical_input(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": "2026-02-02T16:30:00Z"},
        {"timestamp": "2026-02-02T14:30:00Z"},
        {"timestamp": "2026-02-02T16:31:00Z"},
    ]

    first_result = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
        sparse_hour_event_threshold=2,
    )
    second_result = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
        sparse_hour_event_threshold=2,
    )

    assert _result_snapshot(first_result) == _result_snapshot(second_result)
    assert first_result.source_record_positions == [1, 0, 2]


def test_preprocess_timestamp_events_accepts_datetime_values(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC)},
    ]

    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
    )

    assert preprocessed.timestamp_utc_nanos == [_utc_nanos(2026, 2, 2, 14, 30, 0)]


def test_preprocess_timestamp_events_raises_for_naive_datetime_values(
    calendar_registry: CalendarRegistry,
) -> None:
    naive_datetime: datetime = cast(datetime, datetime(2026, 2, 2, 14, 30, 0))
    event_records: list[dict[str, Any]] = [{"timestamp": naive_datetime}]

    with pytest.raises(
        SeasonalityInputValidationError,
        match="naive timestamp",
    ):
        preprocess_timestamp_events(
            event_records,
            exchange_code="XNYS",
            calendar_registry=calendar_registry,
        )


def test_estimate_intraday_seasonality_recovers_synthetic_bimodal_pattern(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = _build_bimodal_intraday_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.OPEN_ONLY,
    )
    estimate = estimate_intraday_seasonality(
        preprocessed,
        settings=IntradaySeasonalityEstimatorSettings(
            minutes_per_bin=30,
            smoothing_kernel=IntradaySmoothingKernel.GAUSSIAN,
            smoothing_bandwidth_bins=1.2,
            minimum_bin_events=1,
            pseudocount=1e-6,
        ),
    )
    open_bin_index: int = 570 // 30
    midday_bin_index: int = 720 // 30
    close_bin_index: int = 930 // 30

    assert estimate.schema_version == INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION
    assert estimate.intraday_factor_by_bin[open_bin_index] > estimate.intraday_factor_by_bin[midday_bin_index]
    assert estimate.intraday_factor_by_bin[close_bin_index] > estimate.intraday_factor_by_bin[midday_bin_index]
    assert sum(estimate.intraday_factor_by_bin) / len(estimate.intraday_factor_by_bin) == pytest.approx(1.0)


def test_estimate_intraday_seasonality_handles_sparse_bins_and_day_boundary_wrap(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = [
        {"timestamp": "2026-02-02T00:00:00-05:00"},
        {"timestamp": "2026-02-02T00:00:30-05:00"},
        {"timestamp": "2026-02-02T23:59:00-05:00"},
        {"timestamp": "2026-02-02T23:59:30-05:00"},
        {"timestamp": "2026-02-03T00:00:00-05:00"},
        {"timestamp": "2026-02-03T23:59:00-05:00"},
    ]

    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )
    estimate = estimate_intraday_seasonality(
        preprocessed,
        settings=IntradaySeasonalityEstimatorSettings(
            minutes_per_bin=1,
            smoothing_kernel=IntradaySmoothingKernel.GAUSSIAN,
            smoothing_bandwidth_bins=2.0,
            minimum_bin_events=2,
            pseudocount=1e-6,
        ),
    )

    assert estimate.bin_count == 1_440
    assert estimate.sparse_bin_count > 1_000
    assert all(intraday_factor > 0.0 for intraday_factor in estimate.intraday_factor_by_bin)
    assert abs(estimate.intraday_factor_by_bin[0] - estimate.intraday_factor_by_bin[1_439]) < 0.4
    assert estimate.intraday_factor_by_bin[0] > estimate.intraday_factor_by_bin[720]


def test_estimate_intraday_seasonality_is_reproducible_for_fixed_input(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = _build_bimodal_intraday_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.OPEN_ONLY,
    )
    settings = IntradaySeasonalityEstimatorSettings(
        minutes_per_bin=15,
        smoothing_kernel=IntradaySmoothingKernel.UNIFORM,
        smoothing_bandwidth_bins=2.0,
        minimum_bin_events=1,
        pseudocount=1e-6,
    )

    first_estimate = estimate_intraday_seasonality(preprocessed, settings=settings)
    second_estimate = estimate_intraday_seasonality(preprocessed, settings=settings)

    assert _intraday_estimate_snapshot(first_estimate) == _intraday_estimate_snapshot(second_estimate)


def test_estimate_day_of_week_seasonality_recovers_synthetic_weekly_pattern(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = _build_weekly_pattern_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )
    estimate = estimate_day_of_week_seasonality(
        preprocessed,
        settings=DayOfWeekSeasonalityEstimatorSettings(
            minimum_bucket_events=2,
            pseudocount=1e-6,
        ),
    )

    assert estimate.schema_version == DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION
    assert estimate.day_of_week_factor[0] > estimate.day_of_week_factor[2]
    assert estimate.day_of_week_factor[4] > estimate.day_of_week_factor[2]
    assert estimate.sparse_bucket_flags[5] == 1
    assert estimate.sparse_bucket_flags[6] == 1
    assert sum(estimate.day_of_week_factor) / len(estimate.day_of_week_factor) == pytest.approx(1.0)


def test_estimate_day_of_year_seasonality_handles_sparse_day_buckets(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = _build_yearly_pattern_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )
    estimate = estimate_day_of_year_seasonality(
        preprocessed,
        settings=DayOfYearSeasonalityEstimatorSettings(
            minimum_bucket_events=5,
            pseudocount=1e-6,
        ),
    )

    assert estimate.schema_version == DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION
    assert estimate.day_of_year_factor[59] > estimate.day_of_year_factor[199]
    assert estimate.sparse_bucket_flags[329] == 1
    assert estimate.sparse_bucket_flags[149] == 1
    assert abs(estimate.day_of_year_factor[329] - estimate.day_of_year_factor[149]) < 1e-10
    assert sum(estimate.day_of_year_factor) / len(estimate.day_of_year_factor) == pytest.approx(1.0)


def test_compose_deterministic_seasonality_signal_is_explicit_and_reproducible(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = _build_weekly_pattern_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )
    intraday_estimate = estimate_intraday_seasonality(
        preprocessed,
        settings=IntradaySeasonalityEstimatorSettings(
            minutes_per_bin=30,
            smoothing_kernel=IntradaySmoothingKernel.GAUSSIAN,
            smoothing_bandwidth_bins=1.2,
            minimum_bin_events=1,
            pseudocount=1e-6,
        ),
    )
    day_of_week_estimate = estimate_day_of_week_seasonality(
        preprocessed,
        settings=DayOfWeekSeasonalityEstimatorSettings(
            minimum_bucket_events=2,
            pseudocount=1e-6,
        ),
    )
    day_of_year_estimate = estimate_day_of_year_seasonality(
        preprocessed,
        settings=DayOfYearSeasonalityEstimatorSettings(
            minimum_bucket_events=1,
            pseudocount=1e-6,
        ),
    )

    first_output = compose_deterministic_seasonality_signal(
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
    )
    second_output = compose_deterministic_seasonality_signal(
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
    )

    assert first_output.schema_version == DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION
    assert _composition_snapshot(first_output) == _composition_snapshot(second_output)
    assert first_output.normalization_policy == "mean_factor_anchor.v1"
    assert first_output.fit_window_event_count == first_output.input_event_count
    assert first_output.target_mean_factor == pytest.approx(1.0)
    assert first_output.post_normalization_mean_factor == pytest.approx(1.0)
    assert first_output.normalization_constant > 0.0

    for event_position in range(first_output.input_event_count):
        expected_log_component = (
            first_output.intraday_log_component_by_event[event_position]
            + first_output.day_of_week_log_component_by_event[event_position]
            + first_output.day_of_year_log_component_by_event[event_position]
            + first_output.special_log_component_by_event[event_position]
            - first_output.normalization_log_offset
        )

        assert first_output.combined_log_component_by_event[event_position] == pytest.approx(
            expected_log_component
        )
        assert first_output.combined_factor_by_event[event_position] == pytest.approx(
            math.exp(expected_log_component)
        )

    assert sum(first_output.combined_factor_by_event) / first_output.input_event_count == pytest.approx(1.0)


def test_compose_deterministic_seasonality_signal_applies_single_and_multiple_overlay_windows(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = _build_weekly_pattern_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )
    intraday_estimate = estimate_intraday_seasonality(preprocessed)
    day_of_week_estimate = estimate_day_of_week_seasonality(preprocessed)
    day_of_year_estimate = estimate_day_of_year_seasonality(preprocessed)
    scheduled_event_overlays = load_scheduled_event_overlay_definitions(
        [
            {
                "overlay_id": "monday_window",
                "start_timestamp_utc": "2026-02-02T15:00:00Z",
                "end_timestamp_utc": "2026-02-02T15:30:00Z",
                "log_adjustment": 0.2,
                "precedence": 1,
                "source_label": "macro_calendar",
            },
            {
                "overlay_id": "friday_window",
                "start_timestamp_utc": "2026-02-06T15:00:00Z",
                "end_timestamp_utc": "2026-02-06T15:30:00Z",
                "log_adjustment": -0.1,
                "precedence": 1,
                "source_label": "macro_calendar",
            },
        ]
    )

    composition = compose_deterministic_seasonality_signal(
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        scheduled_event_overlays=scheduled_event_overlays,
    )

    monday_event_positions = [
        event_position
        for event_position, day_of_week in enumerate(preprocessed.exchange_local_day_of_week)
        if day_of_week == 1
    ]
    friday_event_positions = [
        event_position
        for event_position, day_of_week in enumerate(preprocessed.exchange_local_day_of_week)
        if day_of_week == 5
    ]

    assert composition.configured_overlay_count == 2
    assert composition.enabled_overlay_count == 2
    assert composition.overlays_enabled is True
    assert composition.active_overlay_event_count == len(monday_event_positions) + len(friday_event_positions)
    assert composition.overlay_precedence_policy == "highest_precedence_wins.v1"
    assert composition.overlay_merge_policy == "equal_precedence_additive.v1"
    assert composition.overlay_window_boundary_policy == "start_inclusive_end_exclusive.v1"
    assert composition.configured_overlay_ids_in_order == ["monday_window", "friday_window"]
    assert composition.applied_overlay_ids_in_order == ["monday_window", "friday_window"]

    for event_position in monday_event_positions:
        assert composition.special_log_component_by_event[event_position] == pytest.approx(0.2)
        assert composition.special_overlay_active_flags_by_event[event_position] == 1
        assert composition.active_overlay_ids_by_event[event_position] == "monday_window"

    for event_position in friday_event_positions:
        assert composition.special_log_component_by_event[event_position] == pytest.approx(-0.1)
        assert composition.special_overlay_active_flags_by_event[event_position] == 1
        assert composition.active_overlay_ids_by_event[event_position] == "friday_window"


def test_compose_deterministic_seasonality_signal_applies_overlay_precedence_when_windows_overlap(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = _build_weekly_pattern_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )
    intraday_estimate = estimate_intraday_seasonality(preprocessed)
    day_of_week_estimate = estimate_day_of_week_seasonality(preprocessed)
    day_of_year_estimate = estimate_day_of_year_seasonality(preprocessed)
    scheduled_event_overlays = [
        ScheduledEventOverlayDefinition(
            overlay_id="low_priority",
            start_timestamp_utc="2026-02-02T15:00:00Z",
            end_timestamp_utc="2026-02-02T15:30:00Z",
            log_adjustment=0.1,
            precedence=1,
            source_label="macro_calendar",
        ),
        ScheduledEventOverlayDefinition(
            overlay_id="high_priority",
            start_timestamp_utc="2026-02-02T15:00:00Z",
            end_timestamp_utc="2026-02-02T15:30:00Z",
            log_adjustment=0.3,
            precedence=2,
            source_label="macro_calendar",
        ),
        ScheduledEventOverlayDefinition(
            overlay_id="high_priority_peer",
            start_timestamp_utc="2026-02-02T15:00:00Z",
            end_timestamp_utc="2026-02-02T15:30:00Z",
            log_adjustment=0.2,
            precedence=2,
            source_label="macro_calendar",
        ),
    ]

    composition = compose_deterministic_seasonality_signal(
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        scheduled_event_overlays=scheduled_event_overlays,
    )

    monday_event_positions = [
        event_position
        for event_position, day_of_week in enumerate(preprocessed.exchange_local_day_of_week)
        if day_of_week == 1
    ]
    assert monday_event_positions
    assert composition.applied_overlay_ids_in_order == ["high_priority", "high_priority_peer"]

    for event_position in monday_event_positions:
        assert composition.special_log_component_by_event[event_position] == pytest.approx(0.5)
        assert composition.active_overlay_ids_by_event[event_position] == "high_priority,high_priority_peer"


def test_compose_deterministic_seasonality_signal_is_noop_when_overlays_are_disabled(
    calendar_registry: CalendarRegistry,
) -> None:
    event_records: list[dict[str, Any]] = _build_weekly_pattern_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )
    intraday_estimate = estimate_intraday_seasonality(preprocessed)
    day_of_week_estimate = estimate_day_of_week_seasonality(preprocessed)
    day_of_year_estimate = estimate_day_of_year_seasonality(preprocessed)

    without_overlays = compose_deterministic_seasonality_signal(
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
    )
    with_disabled_overlay = compose_deterministic_seasonality_signal(
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        scheduled_event_overlays=[
            ScheduledEventOverlayDefinition(
                overlay_id="disabled_overlay",
                start_timestamp_utc="2026-02-02T15:00:00Z",
                end_timestamp_utc="2026-02-02T15:30:00Z",
                log_adjustment=1.0,
                precedence=100,
                enabled=False,
                source_label="macro_calendar",
            )
        ],
    )

    assert with_disabled_overlay.active_overlay_event_count == 0
    assert all(flag == 0 for flag in with_disabled_overlay.special_overlay_active_flags_by_event)
    assert without_overlays.special_log_component_by_event == with_disabled_overlay.special_log_component_by_event
    assert without_overlays.combined_log_component_by_event == with_disabled_overlay.combined_log_component_by_event
    assert without_overlays.combined_factor_by_event == with_disabled_overlay.combined_factor_by_event


def test_generate_seasonality_diagnostics_report_emits_component_metrics_and_metadata(
    calendar_registry: CalendarRegistry,
) -> None:
    (
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        composition,
    ) = _build_diagnostics_report_components(calendar_registry)
    report = generate_seasonality_diagnostics_report(
        "dataset_alpha",
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        composition,
    )

    assert report["schema_version"] == SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION
    assert report["dataset_id"] == "dataset_alpha"

    intraday_summary = cast(dict[str, Any], report["intraday_component_summary"])
    day_of_week_summary = cast(dict[str, Any], report["day_of_week_component_summary"])
    day_of_year_summary = cast(dict[str, Any], report["day_of_year_component_summary"])
    fit_quality_metrics = cast(dict[str, Any], report["fit_quality_metrics"])
    reproducibility_metadata = cast(dict[str, Any], report["reproducibility_metadata"])
    input_snapshot = cast(dict[str, Any], reproducibility_metadata["input_snapshot"])
    estimator_configuration = cast(dict[str, Any], reproducibility_metadata["estimator_configuration"])

    assert intraday_summary["component_name"] == "intraday"
    assert day_of_week_summary["component_name"] == "day_of_week"
    assert day_of_year_summary["component_name"] == "day_of_year"
    assert cast(float, fit_quality_metrics["combined_factor_mean"]) > 0.0
    assert cast(float, fit_quality_metrics["normalization_anchor_error"]) <= 1e-12
    assert input_snapshot["snapshot_fingerprint_algorithm"] == "fnv1a64.v1"
    assert len(cast(str, input_snapshot["snapshot_fingerprint_hex"])) == 16
    assert reproducibility_metadata["estimator_version"] == DEFAULT_SEASONALITY_ESTIMATOR_VERSION
    assert estimator_configuration["intraday_schema_version"] == INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION

    overlay_summary = cast(dict[str, Any], report["overlay_component_summary"])
    assert overlay_summary["configured_overlay_count"] == 1
    assert overlay_summary["active_overlay_event_count"] > 0


def test_generate_seasonality_diagnostics_report_validates_required_metadata_fields(
    calendar_registry: CalendarRegistry,
) -> None:
    (
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        composition,
    ) = _build_diagnostics_report_components(calendar_registry)

    with pytest.raises(
        SeasonalityInputValidationError,
        match="non-empty dataset_id",
    ):
        generate_seasonality_diagnostics_report(
            "",
            preprocessed,
            intraday_estimate,
            day_of_week_estimate,
            day_of_year_estimate,
            composition,
        )


def test_generate_seasonality_diagnostics_report_is_deterministic_for_fixed_inputs(
    calendar_registry: CalendarRegistry,
) -> None:
    (
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        composition,
    ) = _build_diagnostics_report_components(calendar_registry)

    first_report = generate_seasonality_diagnostics_report(
        "dataset_alpha",
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        composition,
    )
    second_report = generate_seasonality_diagnostics_report(
        "dataset_alpha",
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        composition,
    )

    assert first_report == second_report


def test_generate_seasonality_diagnostics_reports_batch_supports_multiple_datasets(
    calendar_registry: CalendarRegistry,
) -> None:
    (
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        composition,
    ) = _build_diagnostics_report_components(calendar_registry)
    batch_report = generate_seasonality_diagnostics_reports_batch(
        [
            SeasonalityDiagnosticsBatchInput(
                dataset_id="dataset_alpha",
                preprocessed_events=preprocessed,
                intraday_estimate=intraday_estimate,
                day_of_week_estimate=day_of_week_estimate,
                day_of_year_estimate=day_of_year_estimate,
                composition=composition,
            ),
            SeasonalityDiagnosticsBatchInput(
                dataset_id="dataset_beta",
                preprocessed_events=preprocessed,
                intraday_estimate=intraday_estimate,
                day_of_week_estimate=day_of_week_estimate,
                day_of_year_estimate=day_of_year_estimate,
                composition=composition,
            ),
        ]
    )

    assert batch_report["schema_version"] == SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION
    assert batch_report["report_count"] == 2
    assert batch_report["dataset_ids_in_order"] == ["dataset_alpha", "dataset_beta"]

    reports = cast(list[dict[str, Any]], batch_report["reports"])
    assert reports[0]["schema_version"] == SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION
    assert reports[1]["schema_version"] == SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION

    with pytest.raises(
        SeasonalityInputValidationError,
        match="duplicate dataset id",
    ):
        generate_seasonality_diagnostics_reports_batch(
            [
                SeasonalityDiagnosticsBatchInput(
                    dataset_id="dataset_alpha",
                    preprocessed_events=preprocessed,
                    intraday_estimate=intraday_estimate,
                    day_of_week_estimate=day_of_week_estimate,
                    day_of_year_estimate=day_of_year_estimate,
                    composition=composition,
                ),
                SeasonalityDiagnosticsBatchInput(
                    dataset_id="dataset_alpha",
                    preprocessed_events=preprocessed,
                    intraday_estimate=intraday_estimate,
                    day_of_week_estimate=day_of_week_estimate,
                    day_of_year_estimate=day_of_year_estimate,
                    composition=composition,
                ),
            ]
        )


def _build_diagnostics_report_components(
    calendar_registry: CalendarRegistry,
) -> tuple[
    TimestampEventPreprocessingResult,
    IntradaySeasonalityEstimate,
    DayOfWeekSeasonalityEstimate,
    DayOfYearSeasonalityEstimate,
    DeterministicSeasonalityComposition,
]:
    event_records: list[dict[str, Any]] = _build_weekly_pattern_event_records()
    preprocessed = preprocess_timestamp_events(
        event_records,
        exchange_code="XNYS",
        calendar_registry=calendar_registry,
        session_filter=SessionFilter.ALL,
    )
    intraday_estimate = estimate_intraday_seasonality(preprocessed)
    day_of_week_estimate = estimate_day_of_week_seasonality(preprocessed)
    day_of_year_estimate = estimate_day_of_year_seasonality(preprocessed)
    composition = compose_deterministic_seasonality_signal(
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        scheduled_event_overlays=[
            ScheduledEventOverlayDefinition(
                overlay_id="monday_window",
                start_timestamp_utc="2026-02-02T15:00:00Z",
                end_timestamp_utc="2026-02-02T15:30:00Z",
                log_adjustment=0.2,
                precedence=1,
                enabled=True,
                source_label="macro_calendar",
            )
        ],
    )

    return (
        preprocessed,
        intraday_estimate,
        day_of_week_estimate,
        day_of_year_estimate,
        composition,
    )


def _result_snapshot(result: TimestampEventPreprocessingResult) -> dict[str, Any]:
    return {
        "schema_version": cast(str, result.schema_version),
        "input_schema_version": cast(str, result.input_schema_version),
        "exchange_code": cast(str, result.exchange_code),
        "exchange_timezone_name": cast(str, result.exchange_timezone_name),
        "source_record_positions": list(cast(list[int], result.source_record_positions)),
        "timestamp_utc_nanos": list(cast(list[int], result.timestamp_utc_nanos)),
        "exchange_local_minute_of_day": list(cast(list[int], result.exchange_local_minute_of_day)),
        "exchange_local_day_of_week": bytes(cast(bytes, result.exchange_local_day_of_week)),
        "exchange_local_day_of_year": list(cast(list[int], result.exchange_local_day_of_year)),
        "market_state_codes": bytes(cast(bytes, result.market_state_codes)),
        "missing_hour_bucket_utc_nanos": list(cast(list[int], result.missing_hour_bucket_utc_nanos)),
        "sparse_hour_bucket_utc_nanos": list(cast(list[int], result.sparse_hour_bucket_utc_nanos)),
        "input_record_count": cast(int, result.input_record_count),
        "validated_record_count": cast(int, result.validated_record_count),
        "accepted_record_count": cast(int, result.accepted_record_count),
        "filtered_record_count": cast(int, result.filtered_record_count),
        "first_event_timestamp_utc_nanos": cast(Optional[int], result.first_event_timestamp_utc_nanos),
        "last_event_timestamp_utc_nanos": cast(Optional[int], result.last_event_timestamp_utc_nanos),
        "hour_bucket_count": cast(int, result.hour_bucket_count),
        "missing_hour_bucket_count": cast(int, result.missing_hour_bucket_count),
        "sparse_hour_bucket_count": cast(int, result.sparse_hour_bucket_count),
    }


def _intraday_estimate_snapshot(estimate: IntradaySeasonalityEstimate) -> dict[str, Any]:
    return {
        "schema_version": cast(str, estimate.schema_version),
        "minutes_per_bin": cast(int, estimate.minutes_per_bin),
        "smoothing_kernel": cast(str, estimate.smoothing_kernel),
        "smoothing_bandwidth_bins": cast(float, estimate.smoothing_bandwidth_bins),
        "minimum_bin_events": cast(int, estimate.minimum_bin_events),
        "pseudocount": cast(float, estimate.pseudocount),
        "bin_start_minute_of_day": list(cast(list[int], estimate.bin_start_minute_of_day)),
        "event_count_by_bin": list(cast(list[int], estimate.event_count_by_bin)),
        "sparse_bin_flags": bytes(cast(bytes, estimate.sparse_bin_flags)),
        "smoothed_density_by_bin": list(cast(list[float], estimate.smoothed_density_by_bin)),
        "intraday_log_component_by_bin": list(cast(list[float], estimate.intraday_log_component_by_bin)),
        "intraday_factor_by_bin": list(cast(list[float], estimate.intraday_factor_by_bin)),
        "input_event_count": cast(int, estimate.input_event_count),
        "bin_count": cast(int, estimate.bin_count),
        "zero_event_bin_count": cast(int, estimate.zero_event_bin_count),
        "sparse_bin_count": cast(int, estimate.sparse_bin_count),
    }


def _composition_snapshot(composition: DeterministicSeasonalityComposition) -> dict[str, Any]:
    return {
        "schema_version": cast(str, composition.schema_version),
        "intraday_minutes_per_bin": cast(int, composition.intraday_minutes_per_bin),
        "intraday_bin_index_by_event": list(cast(list[int], composition.intraday_bin_index_by_event)),
        "intraday_log_component_by_event": list(cast(list[float], composition.intraday_log_component_by_event)),
        "day_of_week_log_component_by_event": list(
            cast(list[float], composition.day_of_week_log_component_by_event)
        ),
        "day_of_year_log_component_by_event": list(cast(list[float], composition.day_of_year_log_component_by_event)),
        "special_log_component_by_event": list(cast(list[float], composition.special_log_component_by_event)),
        "special_overlay_active_flags_by_event": bytes(
            cast(bytes, composition.special_overlay_active_flags_by_event)
        ),
        "active_overlay_ids_by_event": list(cast(list[str], composition.active_overlay_ids_by_event)),
        "combined_log_component_by_event": list(cast(list[float], composition.combined_log_component_by_event)),
        "combined_factor_by_event": list(cast(list[float], composition.combined_factor_by_event)),
        "input_event_count": cast(int, composition.input_event_count),
        "normalization_policy": cast(str, composition.normalization_policy),
        "fit_window_event_count": cast(int, composition.fit_window_event_count),
        "target_mean_factor": cast(float, composition.target_mean_factor),
        "pre_normalization_mean_factor": cast(float, composition.pre_normalization_mean_factor),
        "normalization_constant": cast(float, composition.normalization_constant),
        "normalization_log_offset": cast(float, composition.normalization_log_offset),
        "post_normalization_mean_factor": cast(float, composition.post_normalization_mean_factor),
        "mean_anchor_tolerance": cast(float, composition.mean_anchor_tolerance),
        "overlays_enabled": cast(bool, composition.overlays_enabled),
        "configured_overlay_count": cast(int, composition.configured_overlay_count),
        "enabled_overlay_count": cast(int, composition.enabled_overlay_count),
        "active_overlay_event_count": cast(int, composition.active_overlay_event_count),
        "overlay_precedence_policy": cast(str, composition.overlay_precedence_policy),
        "overlay_merge_policy": cast(str, composition.overlay_merge_policy),
        "overlay_window_boundary_policy": cast(str, composition.overlay_window_boundary_policy),
        "configured_overlay_ids_in_order": list(cast(list[str], composition.configured_overlay_ids_in_order)),
        "applied_overlay_ids_in_order": list(cast(list[str], composition.applied_overlay_ids_in_order)),
        "overlay_source_labels_in_order": list(cast(list[str], composition.overlay_source_labels_in_order)),
    }


def _build_bimodal_intraday_event_records() -> list[dict[str, Any]]:
    event_records: list[dict[str, Any]] = []

    for day in [2, 3, 4, 5, 6]:
        for second in range(20):
            event_records.append({"timestamp": f"2026-02-{day:02d}T09:30:{second:02d}-05:00"})
        for second in range(4):
            event_records.append({"timestamp": f"2026-02-{day:02d}T12:00:{second:02d}-05:00"})
        for second in range(18):
            event_records.append({"timestamp": f"2026-02-{day:02d}T15:30:{second:02d}-05:00"})

    return event_records


def _build_weekly_pattern_event_records() -> list[dict[str, Any]]:
    event_records: list[dict[str, Any]] = []
    weekly_pattern = [
        ("2026-02-02", 60),
        ("2026-02-03", 30),
        ("2026-02-04", 10),
        ("2026-02-05", 20),
        ("2026-02-06", 50),
    ]

    for day_value, event_count in weekly_pattern:
        for second in range(event_count):
            event_records.append({"timestamp": f"{day_value}T10:00:{second % 60:02d}-05:00"})

    return event_records


def _build_yearly_pattern_event_records() -> list[dict[str, Any]]:
    event_records: list[dict[str, Any]] = []

    for second in range(60):
        event_records.append({"timestamp": f"2026-03-01T10:00:{second:02d}-05:00"})
    for second in range(20):
        event_records.append({"timestamp": f"2026-07-19T10:00:{second:02d}-04:00"})

    event_records.append({"timestamp": "2026-11-26T10:00:00-05:00"})

    return event_records


def _utc_nanos(
    year: int,
    month: int,
    day: int,
    hour: int,
    minute: int,
    second: int,
) -> int:
    timestamp = datetime(year, month, day, hour, minute, second, tzinfo=UTC)
    return int(timestamp.timestamp() * 1_000_000_000)
