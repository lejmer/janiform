from __future__ import annotations

from collections.abc import Mapping, Sequence
from typing import TYPE_CHECKING, Any, cast

from ...contracts.base import ContractModel
from ..shared import HawkesExcitationState

if TYPE_CHECKING:
    from .path import (
        CoarseSimulationPath,
        CoarseSimulationPathDiagnostics,
        CoarseSimulationPathOutputSchemaMetadata,
        CoarseSimulationPathProvenance,
        CoarseSimulationPathRecord,
    )
    from ...sufficient_statistics import (
        SufficientStatisticsIntervalStatistics,
        SufficientStatisticsReturnComponents,
    )


def coarse_simulation_path_from_validated_payload(
    raw_payload: Mapping[str, Any],
) -> CoarseSimulationPath:
    from .path import CoarseSimulationPath

    records = tuple(
        _coarse_simulation_path_record_from_validated_payload(
            cast(Mapping[str, Any], raw_record)
        )
        for raw_record in cast(Sequence[Any], raw_payload["records"])
    )

    return _trusted_contract_instance(
        CoarseSimulationPath,
        {
            "schema_version": raw_payload["schema_version"],
            "run_request_schema_version": raw_payload["run_request_schema_version"],
            "output_schema_metadata": _coarse_simulation_path_output_schema_metadata_from_validated_payload(
                cast(Mapping[str, Any], raw_payload["output_schema_metadata"])
            ),
            "provenance": _coarse_simulation_path_provenance_from_validated_payload(
                cast(Mapping[str, Any], raw_payload["provenance"])
            ),
            "diagnostics": _coarse_simulation_path_diagnostics_from_validated_payload(
                cast(Mapping[str, Any], raw_payload["diagnostics"])
            ),
            "seed": raw_payload["seed"],
            "start_time_utc_nanos": raw_payload["start_time_utc_nanos"],
            "end_time_utc_nanos": raw_payload["end_time_utc_nanos"],
            "interval_delta": raw_payload["interval_delta"],
            "simulated_interval_count": raw_payload["simulated_interval_count"],
            "initial_excitation_state": _hawkes_excitation_state_from_validated_payload(
                cast(Mapping[str, Any], raw_payload["initial_excitation_state"])
            ),
            "terminal_excitation_state": _hawkes_excitation_state_from_validated_payload(
                cast(Mapping[str, Any], raw_payload["terminal_excitation_state"])
            ),
            "records": records,
        },
    )


def _coarse_simulation_path_record_from_validated_payload(
    raw_payload: Mapping[str, Any],
) -> CoarseSimulationPathRecord:
    from .path import CoarseSimulationPathRecord
    from ...sufficient_statistics import (
        SufficientStatisticsIntervalStatistics,
        SufficientStatisticsReturnComponents,
    )

    return _trusted_contract_instance(
        CoarseSimulationPathRecord,
        {
            "schema_version": raw_payload["schema_version"],
            "interval_index": raw_payload["interval_index"],
            "interval_start_utc_nanos": raw_payload["interval_start_utc_nanos"],
            "interval_end_utc_nanos": raw_payload["interval_end_utc_nanos"],
            "sufficient_statistics": _trusted_contract_instance(
                SufficientStatisticsIntervalStatistics,
                cast(Mapping[str, Any], raw_payload["sufficient_statistics"]),
            ),
            "return_components": _trusted_contract_instance(
                SufficientStatisticsReturnComponents,
                cast(Mapping[str, Any], raw_payload["return_components"]),
            ),
            "core_total_return": raw_payload["core_total_return"],
            "observed_interval_return": raw_payload["observed_interval_return"],
            "observed_log_price_end": raw_payload["observed_log_price_end"],
            "anchor_value_end": raw_payload["anchor_value_end"],
            "deviation_from_anchor_end": raw_payload["deviation_from_anchor_end"],
        },
    )


def _coarse_simulation_path_output_schema_metadata_from_validated_payload(
    raw_payload: Mapping[str, Any],
) -> CoarseSimulationPathOutputSchemaMetadata:
    from .path import CoarseSimulationPathOutputSchemaMetadata

    return _trusted_contract_instance(
        CoarseSimulationPathOutputSchemaMetadata,
        {
            "schema_version": raw_payload["schema_version"],
            "provenance_schema_version": raw_payload["provenance_schema_version"],
            "diagnostics_schema_version": raw_payload["diagnostics_schema_version"],
            "artifact_workflow_policy": raw_payload["artifact_workflow_policy"],
            "environment_metadata_policy": raw_payload["environment_metadata_policy"],
            "required_path_fields": tuple(raw_payload["required_path_fields"]),
            "required_record_fields": tuple(raw_payload["required_record_fields"]),
            "required_provenance_fields": tuple(raw_payload["required_provenance_fields"]),
            "required_diagnostics_fields": tuple(raw_payload["required_diagnostics_fields"]),
        },
    )


def _coarse_simulation_path_provenance_from_validated_payload(
    raw_payload: Mapping[str, Any],
) -> CoarseSimulationPathProvenance:
    from .path import CoarseSimulationPathProvenance

    return _trusted_contract_instance(
        CoarseSimulationPathProvenance,
        {
            "schema_version": raw_payload["schema_version"],
            "artifact_workflow_policy": raw_payload["artifact_workflow_policy"],
            "environment_metadata_policy": raw_payload["environment_metadata_policy"],
            "interval_timing_policy": raw_payload["interval_timing_policy"],
            "interval_identifier_policy": raw_payload["interval_identifier_policy"],
            "interval_factor_alignment_policy": raw_payload["interval_factor_alignment_policy"],
            "compatible_interval_grid_policy": raw_payload["compatible_interval_grid_policy"],
            "interval_factor_policy": raw_payload["interval_factor_policy"],
            "transform_slice_sampling_policy": raw_payload["transform_slice_sampling_policy"],
            "boundary_state_update_policy": raw_payload["boundary_state_update_policy"],
            "interval_return_generation_policy": raw_payload["interval_return_generation_policy"],
            "interval_return_component_routing_policy": raw_payload[
                "interval_return_component_routing_policy"
            ],
            "batch_seed_partition_policy": raw_payload["batch_seed_partition_policy"],
        },
    )


def _coarse_simulation_path_diagnostics_from_validated_payload(
    raw_payload: Mapping[str, Any],
) -> CoarseSimulationPathDiagnostics:
    from .path import CoarseSimulationPathDiagnostics

    return _trusted_contract_instance(
        CoarseSimulationPathDiagnostics,
        {
            "schema_version": raw_payload["schema_version"],
            "simulated_interval_count": raw_payload["simulated_interval_count"],
            "first_interval_index": raw_payload["first_interval_index"],
            "last_interval_index": raw_payload["last_interval_index"],
            "first_interval_start_utc_nanos": raw_payload["first_interval_start_utc_nanos"],
            "last_interval_end_utc_nanos": raw_payload["last_interval_end_utc_nanos"],
            "cumulative_a_plus": raw_payload["cumulative_a_plus"],
            "cumulative_a_minus": raw_payload["cumulative_a_minus"],
            "cumulative_n_plus": raw_payload["cumulative_n_plus"],
            "cumulative_n_minus": raw_payload["cumulative_n_minus"],
            "cumulative_trading_time_return": raw_payload["cumulative_trading_time_return"],
            "cumulative_open_jump_return": raw_payload["cumulative_open_jump_return"],
            "cumulative_closed_gap_return": raw_payload["cumulative_closed_gap_return"],
            "cumulative_total_return": raw_payload["cumulative_total_return"],
            "cumulative_observed_return": raw_payload["cumulative_observed_return"],
            "max_abs_total_return": raw_payload["max_abs_total_return"],
            "max_abs_deviation_from_anchor": raw_payload["max_abs_deviation_from_anchor"],
            "terminal_observed_log_price": raw_payload["terminal_observed_log_price"],
            "terminal_deviation_from_anchor": raw_payload["terminal_deviation_from_anchor"],
        },
    )


def _hawkes_excitation_state_from_validated_payload(
    raw_payload: Mapping[str, Any],
) -> HawkesExcitationState:
    return _trusted_contract_instance(
        HawkesExcitationState,
        {
            "y_plus_plus": raw_payload["y_plus_plus"],
            "y_plus_minus": raw_payload["y_plus_minus"],
            "y_minus_plus": raw_payload["y_minus_plus"],
            "y_minus_minus": raw_payload["y_minus_minus"],
        },
    )


def _trusted_contract_instance[ContractModelT: ContractModel](
    contract_type: type[ContractModelT],
    field_values: Mapping[str, Any],
) -> ContractModelT:
    # Rust has already validated this payload, so hydration avoids duplicate Python validation.
    instance = object.__new__(contract_type)
    for field_name, field_value in field_values.items():
        object.__setattr__(instance, field_name, field_value)

    return cast(ContractModelT, instance)
