from __future__ import annotations

from collections.abc import Mapping
from datetime import UTC, datetime, timedelta
from typing import TYPE_CHECKING, Any, NoReturn, cast

from ...._timestamps import nanoseconds_to_hours
from ....calendar import CalendarRegistry
from ....hourly_factors.workflow import _parse_interval_duration_nanos
from ....mean_reversion import MeanReversionFittedParameters
from ....pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration
from ....simulation.coarse import NewsProcessConfiguration
from ....simulation.shared import HawkesKernelParameters
from ...errors import calibration_error
from ...market_data.contracts import (
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
)
from ..state import CurrentStateEstimationRequest
from .input_inference import tick_data_input_inference_from_model_metadata
from .shared import PreparedModelRuntimeFactors

if TYPE_CHECKING:
    from ....model import JaniformModel


def mean_reversion_fitted_parameters_for_model(model: JaniformModel) -> MeanReversionFittedParameters:
    return MeanReversionFittedParameters.from_mapping(
        dict(
            require_payload_mapping(
                return_components_fitted_parameters_payload(model),
                "mean_reversion_fitted_parameters",
                "return_components.fitted_parameters.mean_reversion_fitted_parameters",
            )
        ),
        field_path="return_components.fitted_parameters.mean_reversion_fitted_parameters",
    )


def prepare_model_runtime_factors_for_model(
    model: JaniformModel,
    *,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    prefer_joint_pricing_block: bool = False,
) -> PreparedModelRuntimeFactors:
    require_runtime_ready_model(model)
    if start_time_utc.tzinfo is None:
        raise calibration_error("start_time_utc", "must be timezone-aware")

    interval_duration_nanos = _parse_interval_duration_nanos(interval, field_name="interval")

    range_end_utc = start_time_utc + timedelta(
        seconds=(interval_duration_nanos * horizon_interval_count) / 1_000_000_000
    )

    calendar_registry = CalendarRegistry(model.session_resolution.resolved_session_definition_json)

    exchange_code = model.session_resolution.exchange_code

    return_components_fitted_parameters = return_components_fitted_parameters_payload(model)

    nig_configuration_payload = (
        pricing_nig_shared_plus_shift_configuration_payload(model)
        if prefer_joint_pricing_block
        else require_payload_mapping(
            return_components_fitted_parameters,
            "nig_shared_plus_shift_configuration",
            "return_components.fitted_parameters.nig_shared_plus_shift_configuration",
        )
    )

    news_process_configuration = NewsProcessConfiguration.from_mapping(
        require_payload_mapping(
            return_components_fitted_parameters,
            "news_process_configuration",
            "return_components.fitted_parameters.news_process_configuration",
        )
    )

    shared_mark_law = require_payload_mapping(
        news_process_configuration.to_payload(),
        "shared_mark_law",
        "return_components.fitted_parameters.news_process_configuration.shared_mark_law",
    )

    return PreparedModelRuntimeFactors(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        interval_duration_nanos=interval_duration_nanos,
        interval_delta=nanoseconds_to_hours(interval_duration_nanos),
        range_end_utc=range_end_utc,
        kernel_parameters=hawkes_kernel_parameters_for_model(model),
        nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration.from_mapping(
            nig_configuration_payload,
            field_path="return_components.fitted_parameters.nig_shared_plus_shift_configuration",
        ),
        gamma_severity_mark_configuration=GammaSeverityMarkConfiguration.from_mapping(
            require_payload_mapping(
                shared_mark_law,
                "gamma_severity_marks",
                "return_components.fitted_parameters.news_process_configuration.shared_mark_law.gamma_severity_marks",
            ),
            field_path="return_components.fitted_parameters.news_process_configuration.shared_mark_law.gamma_severity_marks",
        ),
        news_process_configuration=news_process_configuration,
        return_mark_hour_of_week_factors=require_payload_numeric_sequence(
            require_payload_mapping(
                return_components_fitted_parameters,
                "return_mark_hour_of_week_scale_profile",
                "return_components.fitted_parameters.return_mark_hour_of_week_scale_profile",
            ),
            "hour_of_week_factors",
            "return_components.fitted_parameters.return_mark_hour_of_week_scale_profile.hour_of_week_factors",
        ),
    )


def require_runtime_ready_model(model: JaniformModel) -> None:
    if model.workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
        raise RuntimeError(
            "JaniformModel runtime methods require a fitted model with Hawkes and return-components stages"
        )

    hawkes_fitted_parameters_payload(model)

    return_components_fitted_parameters_payload(model)


def hawkes_kernel_parameters_for_model(model: JaniformModel) -> HawkesKernelParameters:
    return HawkesKernelParameters.from_mapping(
        require_payload_mapping(
            hawkes_fitted_parameters_payload(model),
            "kernel_parameters",
            "hawkes.fitted_parameters.kernel_parameters",
        ),
        field_path="hawkes.fitted_parameters.kernel_parameters",
    )


def current_state_tick_data_column_mapping_for_model(
    model: JaniformModel,
    *,
    request: CurrentStateEstimationRequest,
) -> CalibrationTickDataColumnMapping:
    if request.tick_data_column_mapping is not None:
        column_mapping = request.tick_data_column_mapping
    else:
        input_inference = tick_data_input_inference_from_model_metadata(model.metadata)

        price_column = input_inference.price_column
        trade_sign_column = input_inference.trade_sign_column

        if price_column is None:
            raise calibration_error(
                "metadata.input_inference.tick_data_column_mapping.price_column",
                "must be present so current-state estimation can read observed tick prices",
            )

        if trade_sign_column is None:
            raise calibration_error(
                "metadata.input_inference.tick_data_column_mapping.trade_sign_column",
                "must be present so current-state estimation can read observed trade signs",
            )

        column_mapping = CalibrationTickDataColumnMapping(
            timestamp_column=input_inference.timestamp_column,
            price_column=price_column,
            trade_sign_column=trade_sign_column,
        )

    if column_mapping.price_column is None:
        raise calibration_error(
            "tick_data_column_mapping.price_column",
            "is required for current-state estimation",
        )

    if column_mapping.trade_sign_column is None:
        raise calibration_error(
            "tick_data_column_mapping.trade_sign_column",
            "is required for current-state estimation",
        )

    return column_mapping


def seasonality_parameters_payload(model: JaniformModel) -> Mapping[str, Any]:
    raw_seasonality = model.parameters.get("seasonality")
    if not isinstance(raw_seasonality, Mapping):
        raise RuntimeError("JaniformModel does not contain compact seasonality parameters")

    return raw_seasonality


def hawkes_fitted_parameters_payload(model: JaniformModel) -> Mapping[str, Any]:
    return require_payload_mapping(
        finalization_fitted_parameters_payload(model),
        "hawkes_fitted_parameters",
        "finalization.fitted_parameters.hawkes_fitted_parameters",
    )


def return_components_fitted_parameters_payload(model: JaniformModel) -> Mapping[str, Any]:
    return require_payload_mapping(
        finalization_fitted_parameters_payload(model),
        "return_components_fitted_parameters",
        "finalization.fitted_parameters.return_components_fitted_parameters",
    )


def pricing_nig_shared_plus_shift_configuration_payload(model: JaniformModel) -> Mapping[str, Any]:
    pricing_block = finalization_fitted_parameters_payload(model).get(
        "pricing_block_fitted_parameters"
    )
    if isinstance(pricing_block, Mapping):
        nig_configuration = pricing_block.get("nig_shared_plus_shift_configuration")
        if isinstance(nig_configuration, Mapping):
            return nig_configuration

    return require_payload_mapping(
        return_components_fitted_parameters_payload(model),
        "nig_shared_plus_shift_configuration",
        "return_components.fitted_parameters.nig_shared_plus_shift_configuration",
    )


def finalization_fitted_parameters_payload(model: JaniformModel) -> Mapping[str, Any]:
    finalization_parameters = model.parameters.get("finalization")
    if not isinstance(finalization_parameters, Mapping):
        raise RuntimeError("JaniformModel does not contain finalization parameters")

    fitted_parameters = finalization_parameters.get("fitted_parameters")
    if not isinstance(fitted_parameters, Mapping):
        raise RuntimeError("JaniformModel is missing finalization fitted parameters")

    return fitted_parameters


def resolved_known_news_event_timestamps(model: JaniformModel) -> tuple[datetime, ...]:
    return tuple(
        event.scheduled_at.astimezone(UTC)
        for event in model.resolved_configuration.known_news.configuration.events
    )


def surprise_baseline_intensity(model: JaniformModel) -> float:
    news_process_configuration = require_payload_mapping(
        return_components_fitted_parameters_payload(model),
        "news_process_configuration",
        "return_components.fitted_parameters.news_process_configuration",
    )

    arrivals = require_payload_mapping(
        news_process_configuration,
        "arrivals",
        "return_components.fitted_parameters.news_process_configuration.arrivals",
    )

    surprise = require_payload_mapping(
        arrivals,
        "surprise",
        "return_components.fitted_parameters.news_process_configuration.arrivals.surprise",
    )

    baseline_intensity = surprise.get("baseline_intensity")
    if not isinstance(baseline_intensity, int | float) or isinstance(baseline_intensity, bool):
        raise RuntimeError(
            "JaniformModel is missing a numeric surprise baseline intensity in the fitted model payload"
        )

    return float(baseline_intensity)


def require_payload_mapping(payload: Mapping[str, Any], field_name: str, field_path: str) -> Mapping[str, Any]:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, Mapping):
        raise RuntimeError(f"JaniformModel payload field `{field_path}` must be an object")

    return raw_value


def require_payload_numeric_value(payload: Mapping[str, Any], field_name: str, field_path: str) -> float:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, int | float) or isinstance(raw_value, bool):
        raise RuntimeError(f"JaniformModel payload field `{field_path}` must be numeric")

    return float(raw_value)


def require_payload_numeric_sequence(
    payload: Mapping[str, Any],
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    raw_value = payload.get(field_name)
    if isinstance(raw_value, str) or not isinstance(raw_value, list | tuple):
        raise RuntimeError(f"JaniformModel payload field `{field_path}` must be a sequence")

    values: list[float] = []
    for entry_index, entry_value in enumerate(raw_value):
        if not isinstance(entry_value, int | float) or isinstance(entry_value, bool):
            raise RuntimeError(f"JaniformModel payload field `{field_path}[{entry_index}]` must be numeric")

        values.append(float(entry_value))

    if not values:
        raise RuntimeError(f"JaniformModel payload field `{field_path}` must not be empty")

    return tuple(values)


def raise_invalid_weekly_surprise_multiplier(bucket_index: int) -> NoReturn:
    raise RuntimeError(
        "JaniformModel payload field `return_components.fitted_parameters.surprise_deterministic_intensity_profile."
        f"weekly_hour_utc_multipliers[{bucket_index}]` must be numeric"
    )
