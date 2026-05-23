from __future__ import annotations

from collections.abc import Mapping
from copy import deepcopy
from typing import Any

from ....mean_reversion import MeanReversionFittedParameters
from ...errors import calibration_error
from .shared import MODEL_PERSISTENCE_SCHEMA_VERSION


def validate_model_persistence_schema_version(raw_schema_version: Any) -> None:
    if raw_schema_version == MODEL_PERSISTENCE_SCHEMA_VERSION:
        return

    raise calibration_error(
        "schema_version",
        f"must be `{MODEL_PERSISTENCE_SCHEMA_VERSION}`",
    )


def normalize_model_parameters(raw_parameters: Mapping[str, Any]) -> dict[str, Any]:
    parameters = deepcopy(dict(raw_parameters))
    raw_seasonality = parameters.get("seasonality")
    if isinstance(raw_seasonality, dict):
        for field_name in (
            "intraday_factor_by_bin",
            "day_of_week_factor",
            "hour_of_week_factor",
            "day_of_year_factor",
        ):
            raw_value = raw_seasonality.get(field_name)
            if isinstance(raw_value, list):
                raw_seasonality[field_name] = tuple(raw_value)

    normalize_mean_reversion_model_parameters(parameters)

    return parameters


def normalize_mean_reversion_model_parameters(parameters: dict[str, Any]) -> None:
    raw_finalization = parameters.get("finalization")
    if not isinstance(raw_finalization, Mapping):
        return

    raw_fitted_parameters = raw_finalization.get("fitted_parameters")
    if isinstance(raw_fitted_parameters, Mapping):
        raw_return_components_fitted_parameters = raw_fitted_parameters.get(
            "return_components_fitted_parameters"
        )
        if isinstance(raw_return_components_fitted_parameters, Mapping):
            normalize_return_components_mean_reversion_block(
                raw_return_components_fitted_parameters,
                field_path="parameters.finalization.fitted_parameters.return_components_fitted_parameters",
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
