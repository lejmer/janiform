"""Internal market-surface pricing workflow helpers."""

from __future__ import annotations

import os
from concurrent.futures import FIRST_COMPLETED, Future, ProcessPoolExecutor, wait
from dataclasses import dataclass
from datetime import datetime
from multiprocessing import Manager
from typing import TYPE_CHECKING, Any, Optional

import pandas

from ......_workflow_progress import JaniformOutputController, WorkflowProgressMode
from ......contracts.base import require_finite_scalar
from ......pricing import VanillaOptionPayoffType
from .....errors import calibration_error
from .....market_data.contracts import CalibrationOptionSurfaceColumnMapping, CalibrationTickDataColumnMapping
from ...market_snapshot._workflow.context import (
    PreparedObservedOptionMarketContext,
    prepare_observed_option_market_context,
)
from ...market_snapshot._workflow.loading import filter_observed_tick_data, observed_spot_from_tick_data
from ...market_snapshot._workflow.pricing import price_prepared_snapshot_groups_for_model
from ...market_snapshot._workflow.preparation import (
    PreparedSnapshotGroupInput,
    prepare_selected_expiry_snapshot_groups_by_valuation_time,
    prepare_selected_expiry_history_frame,
    prepare_snapshot_groups,
)
from .progress import (
    SingleExpirySurfaceSliceProgressRow,
    SingleExpirySurfaceSliceWorkerProgress,
    build_single_expiry_surface_progress_panel,
    complete_single_expiry_surface_slice_progress_row,
    drain_single_expiry_surface_progress_queue,
    fail_single_expiry_surface_slice_progress_row,
)
from ...market_snapshot.result import JaniformObservedOptionSnapshotPricingResult
from ...market_surface.result import (
    JaniformObservedOptionSurfaceNode,
    JaniformObservedOptionSurfacePricingResult,
)

if TYPE_CHECKING:
    from ....model import JaniformModel
    from ...market_snapshot.inputs import TabularFrameInput


@dataclass(frozen=True, slots=True)
class PreparedSingleExpirySurfaceSliceInput:
    valuation_time_utc: datetime
    filtered_tick_data: pandas.DataFrame
    spot: float
    prepared_snapshot_groups: tuple[PreparedSnapshotGroupInput, ...]


def price_option_surface_for_model(
    model: JaniformModel,
    *,
    tick_data: TabularFrameInput,
    option_surface: TabularFrameInput,
    valuation_time_utc: Optional[datetime] = None,
    option_type: Optional[VanillaOptionPayoffType | str] = None,
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = None,
    risk_free_rate: float = 0.0,
    dividend_yield: float = 0.0,
    valuation_count: Optional[int] = None,
    use_all_valuation_times: bool = False,
    cpu_count: Optional[int] = None,
    silent: bool = False,
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
) -> JaniformObservedOptionSurfacePricingResult:
    require_finite_scalar(risk_free_rate, "risk_free_rate", calibration_error)
    require_finite_scalar(dividend_yield, "dividend_yield", calibration_error)

    if valuation_count is not None and valuation_count <= 0:
        raise calibration_error(
            "valuation_count",
            "must be strictly positive when provided",
        )
    if use_all_valuation_times and valuation_count is not None:
        raise calibration_error(
            "valuation_count",
            "cannot be combined with use_all_valuation_times",
        )
    if cpu_count is not None and cpu_count <= 0:
        raise calibration_error(
            "cpu_count",
            "must be strictly positive when provided",
        )

    context = prepare_observed_option_market_context(
        model,
        tick_data=tick_data,
        option_surface=option_surface,
        valuation_time_utc=valuation_time_utc,
        option_type=option_type,
        option_surface_column_mapping=option_surface_column_mapping,
    )

    prepared_snapshot_groups = prepare_snapshot_groups(
        option_surface_frame=context.option_surface_frame,
        resolved_option_columns=context.resolved_option_columns,
        valuation_time_utc=context.valuation_time_utc,
        option_type=context.option_type,
    )

    expiry_count = len({group.expiry_timestamp_utc for group in prepared_snapshot_groups})
    if expiry_count > 1:
        snapshot_result = price_prepared_snapshot_groups_for_model(
            model,
            valuation_time_utc=context.valuation_time_utc,
            spot=context.spot,
            prepared_snapshot_groups=prepared_snapshot_groups,
            filtered_tick_data=context.filtered_tick_data,
            resolved_tick_column_mapping=context.resolved_tick_column_mapping,
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            cpu_count=cpu_count,
            silent=silent,
            progress_mode=progress_mode,
        )

        return build_observed_option_surface_pricing_result(
            valuation_time_utc=context.valuation_time_utc,
            snapshot_results=(snapshot_result,),
        )

    selected_expiry_timestamp_utc = prepared_snapshot_groups[0].expiry_timestamp_utc

    selected_expiry_history = prepare_selected_expiry_history_frame(
        option_surface_frame=context.option_surface_frame,
        resolved_option_columns=context.resolved_option_columns,
        valuation_time_utc=context.valuation_time_utc,
        option_type=context.option_type,
        selected_expiry_timestamp_utc=selected_expiry_timestamp_utc,
    )

    valuation_times = resolve_single_expiry_surface_valuation_times(
        selected_expiry_history=selected_expiry_history,
        valuation_time_utc=context.valuation_time_utc,
        retained_snapshot_valuation_count=_retained_snapshot_valuation_count(
            prepared_snapshot_groups=prepared_snapshot_groups,
        ),
        valuation_count=valuation_count,
        use_all_valuation_times=use_all_valuation_times,
    )

    prepared_snapshot_groups_by_time = prepare_selected_expiry_snapshot_groups_by_valuation_time(
        selected_expiry_history=selected_expiry_history,
        valuation_times_utc=valuation_times,
        resolved_option_columns=context.resolved_option_columns,
    )

    prepared_slice_inputs_by_time = {
        panel_valuation_time_utc: prepare_single_expiry_surface_slice_input(
            context=context,
            panel_valuation_time_utc=panel_valuation_time_utc,
            prepared_snapshot_groups=prepared_snapshot_groups_by_time[panel_valuation_time_utc],
        )
        for panel_valuation_time_utc in valuation_times
    }

    total_cpu_count = max(1, os.cpu_count() or 1) if cpu_count is None else cpu_count
    surface_parallelism = min(total_cpu_count, len(valuation_times))
    pricing_cpu_count = max(1, total_cpu_count // surface_parallelism)
    use_surface_progress = surface_parallelism > 1
    worker_silent = silent if not use_surface_progress else True

    if surface_parallelism <= 1:
        snapshot_results = _price_single_expiry_surface_slices_serial(
            model=model,
            resolved_tick_column_mapping=context.resolved_tick_column_mapping,
            valuation_times=valuation_times,
            prepared_slice_inputs_by_time=prepared_slice_inputs_by_time,
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            pricing_cpu_count=pricing_cpu_count,
            worker_silent=worker_silent,
            progress_mode=progress_mode,
        )
    else:
        snapshot_results = _price_single_expiry_surface_slices_parallel(
            model=model,
            resolved_tick_column_mapping=context.resolved_tick_column_mapping,
            valuation_times=valuation_times,
            prepared_slice_inputs_by_time=prepared_slice_inputs_by_time,
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            surface_parallelism=surface_parallelism,
            total_cpu_count=total_cpu_count,
            pricing_cpu_count=pricing_cpu_count,
            worker_silent=worker_silent,
            silent=silent,
            progress_mode=progress_mode,
        )

    return build_observed_option_surface_pricing_result(
        valuation_time_utc=context.valuation_time_utc,
        snapshot_results=snapshot_results,
    )


def resolve_single_expiry_surface_valuation_times(
    *,
    selected_expiry_history: pandas.DataFrame,
    valuation_time_utc: datetime,
    retained_snapshot_valuation_count: int,
    valuation_count: Optional[int],
    use_all_valuation_times: bool,
) -> tuple[datetime, ...]:
    full_history_valuation_times = tuple(
        pandas.Timestamp(quote_timestamp).tz_convert("UTC").to_pydatetime()
        for quote_timestamp in (
            selected_expiry_history["_quote_timestamp_utc"]
            .drop_duplicates()
            .sort_values(kind="stable")
        )
    )
    if use_all_valuation_times:
        return full_history_valuation_times

    sample_count = (
        valuation_count
        if valuation_count is not None
        else retained_snapshot_valuation_count
    )
    sample_count = min(sample_count, len(full_history_valuation_times))

    if sample_count == len(full_history_valuation_times):
        valuation_times = full_history_valuation_times
    elif sample_count == 1:
        valuation_times = (valuation_time_utc,)
    else:
        selected_positions = {
            round(
                selection_index * (len(full_history_valuation_times) - 1)
                / (sample_count - 1)
            )
            for selection_index in range(sample_count)
        }

        valuation_times = tuple(
            full_history_valuation_times[position]
            for position in sorted(selected_positions)
        )

    if valuation_times[-1] != valuation_time_utc:
        valuation_times = (*valuation_times[:-1], valuation_time_utc)

    return valuation_times


def build_observed_option_surface_pricing_result(
    *,
    valuation_time_utc: datetime,
    snapshot_results: tuple[JaniformObservedOptionSnapshotPricingResult, ...],
) -> JaniformObservedOptionSurfacePricingResult:
    nodes: list[JaniformObservedOptionSurfaceNode] = []
    for snapshot_result in snapshot_results:
        for snapshot_node in snapshot_result.nodes:
            nodes.append(
                JaniformObservedOptionSurfaceNode(
                    valuation_time_utc=snapshot_result.valuation_time_utc,
                    quote_timestamp_utc=snapshot_node.quote_timestamp_utc,
                    expiry_timestamp_utc=snapshot_node.expiry_timestamp_utc,
                    strike=snapshot_node.strike,
                    option_type=snapshot_node.option_type,
                    risk_free_rate=snapshot_node.risk_free_rate,
                    dividend_yield=snapshot_node.dividend_yield,
                    maturity_interval=snapshot_node.maturity_interval,
                    observed_bid_price=snapshot_node.observed_bid_price,
                    observed_ask_price=snapshot_node.observed_ask_price,
                    observed_mid_price=snapshot_node.observed_mid_price,
                    model_price=snapshot_node.model_price,
                    model_implied_volatility=snapshot_node.model_implied_volatility,
                    implied_volatility_inversion_status=snapshot_node.implied_volatility_inversion_status,
                    pricing_status=snapshot_node.pricing_status,
                    pricing_failure_stage=snapshot_node.pricing_failure_stage,
                    pricing_failure_reason=snapshot_node.pricing_failure_reason,
                    forward_price=snapshot_node.forward_price,
                    discount_factor=snapshot_node.discount_factor,
                    boundary_state_norm=snapshot_node.boundary_state_norm,
                )
            )

    return JaniformObservedOptionSurfacePricingResult(
        valuation_time_utc=valuation_time_utc,
        nodes=tuple(nodes),
        raw_results=snapshot_results,
    )


def _price_single_expiry_surface_slices_serial(
    *,
    model: JaniformModel,
    resolved_tick_column_mapping: CalibrationTickDataColumnMapping,
    valuation_times: tuple[datetime, ...],
    prepared_slice_inputs_by_time: dict[datetime, PreparedSingleExpirySurfaceSliceInput],
    risk_free_rate: float,
    dividend_yield: float,
    pricing_cpu_count: int,
    worker_silent: bool,
    progress_mode: WorkflowProgressMode | str,
) -> tuple[JaniformObservedOptionSnapshotPricingResult, ...]:
    return tuple(
        _price_single_expiry_surface_slice(
            model=model,
            resolved_tick_column_mapping=resolved_tick_column_mapping,
            slice_input=prepared_slice_inputs_by_time[panel_valuation_time_utc],
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            cpu_count=pricing_cpu_count,
            silent=worker_silent,
            progress_mode=progress_mode,
        )
        for panel_valuation_time_utc in valuation_times
    )


def _price_single_expiry_surface_slices_parallel(
    *,
    model: JaniformModel,
    resolved_tick_column_mapping: CalibrationTickDataColumnMapping,
    valuation_times: tuple[datetime, ...],
    prepared_slice_inputs_by_time: dict[datetime, PreparedSingleExpirySurfaceSliceInput],
    risk_free_rate: float,
    dividend_yield: float,
    surface_parallelism: int,
    total_cpu_count: int,
    pricing_cpu_count: int,
    worker_silent: bool,
    silent: bool,
    progress_mode: WorkflowProgressMode | str,
) -> tuple[JaniformObservedOptionSnapshotPricingResult, ...]:
    output_controller = JaniformOutputController(
        suppress_info_output=silent,
        progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
        default_progress_mode=(WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN),
    )

    with output_controller.progress("price") as progress:
        rows_by_time = {
            panel_valuation_time_utc: SingleExpirySurfaceSliceProgressRow(
                valuation_time_utc=panel_valuation_time_utc,
            )
            for panel_valuation_time_utc in valuation_times
        }

        _update_single_expiry_surface_progress(
            progress=progress,
            rows_by_time=rows_by_time,
            completed_valuation_count=0,
            active_count=len(valuation_times),
            valuation_count=len(valuation_times),
            surface_parallelism=surface_parallelism,
            total_cpu_count=total_cpu_count,
            pricing_cpu_count=pricing_cpu_count,
            detail="pricing observed option surface",
        )

        with Manager() as manager:
            progress_queue = manager.Queue()

            with ProcessPoolExecutor(max_workers=surface_parallelism) as executor:
                futures = _submit_single_expiry_surface_slice_futures(
                    executor=executor,
                    model=model,
                    resolved_tick_column_mapping=resolved_tick_column_mapping,
                    valuation_times=valuation_times,
                    prepared_slice_inputs_by_time=prepared_slice_inputs_by_time,
                    risk_free_rate=risk_free_rate,
                    dividend_yield=dividend_yield,
                    pricing_cpu_count=pricing_cpu_count,
                    worker_silent=worker_silent,
                    progress_mode=progress_mode,
                    progress_queue=progress_queue,
                )
                snapshot_results_by_time = _collect_parallel_single_expiry_surface_slice_results(
                    progress=progress,
                    progress_queue=progress_queue,
                    rows_by_time=rows_by_time,
                    futures_by_time=futures,
                    valuation_count=len(valuation_times),
                    surface_parallelism=surface_parallelism,
                    total_cpu_count=total_cpu_count,
                    pricing_cpu_count=pricing_cpu_count,
                )

    return tuple(
        snapshot_results_by_time[panel_valuation_time_utc]
        for panel_valuation_time_utc in valuation_times
    )


def _submit_single_expiry_surface_slice_futures(
    *,
    executor: ProcessPoolExecutor,
    model: JaniformModel,
    resolved_tick_column_mapping: CalibrationTickDataColumnMapping,
    valuation_times: tuple[datetime, ...],
    prepared_slice_inputs_by_time: dict[datetime, PreparedSingleExpirySurfaceSliceInput],
    risk_free_rate: float,
    dividend_yield: float,
    pricing_cpu_count: int,
    worker_silent: bool,
    progress_mode: WorkflowProgressMode | str,
    progress_queue: Any,
) -> dict[Future[JaniformObservedOptionSnapshotPricingResult], datetime]:
    return {
        executor.submit(
            _price_single_expiry_surface_slice,
            model=model,
            resolved_tick_column_mapping=resolved_tick_column_mapping,
            slice_input=prepared_slice_inputs_by_time[panel_valuation_time_utc],
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            cpu_count=pricing_cpu_count,
            silent=worker_silent,
            progress_mode=progress_mode,
            progress_queue=progress_queue,
        ): panel_valuation_time_utc
        for panel_valuation_time_utc in valuation_times
    }


def _collect_parallel_single_expiry_surface_slice_results(
    *,
    progress: Any,
    progress_queue: Any,
    rows_by_time: dict[datetime, SingleExpirySurfaceSliceProgressRow],
    futures_by_time: dict[Future[JaniformObservedOptionSnapshotPricingResult], datetime],
    valuation_count: int,
    surface_parallelism: int,
    total_cpu_count: int,
    pricing_cpu_count: int,
) -> dict[datetime, JaniformObservedOptionSnapshotPricingResult]:
    completed_valuation_count = 0
    snapshot_results_by_time: dict[datetime, JaniformObservedOptionSnapshotPricingResult] = {}
    pending_futures = set(futures_by_time)

    while pending_futures:
        completed_futures, pending_futures = wait(
            pending_futures,
            timeout=0.1,
            return_when=FIRST_COMPLETED,
        )

        drain_single_expiry_surface_progress_queue(
            progress_queue=progress_queue,
            rows_by_time=rows_by_time,
        )

        for future in completed_futures:
            panel_valuation_time_utc = futures_by_time[future]
            row = rows_by_time[panel_valuation_time_utc]

            try:
                snapshot_results_by_time[panel_valuation_time_utc] = future.result()
            except Exception:
                fail_single_expiry_surface_slice_progress_row(row)

                _update_single_expiry_surface_progress(
                    progress=progress,
                    rows_by_time=rows_by_time,
                    completed_valuation_count=completed_valuation_count,
                    active_count=len(pending_futures),
                    valuation_count=valuation_count,
                    surface_parallelism=surface_parallelism,
                    total_cpu_count=total_cpu_count,
                    pricing_cpu_count=pricing_cpu_count,
                )

                raise

            complete_single_expiry_surface_slice_progress_row(row)

            completed_valuation_count += 1

        _update_single_expiry_surface_progress(
            progress=progress,
            rows_by_time=rows_by_time,
            completed_valuation_count=completed_valuation_count,
            active_count=len(pending_futures),
            valuation_count=valuation_count,
            surface_parallelism=surface_parallelism,
            total_cpu_count=total_cpu_count,
            pricing_cpu_count=pricing_cpu_count,
        )

    return snapshot_results_by_time


def _update_single_expiry_surface_progress(
    *,
    progress: Any,
    rows_by_time: dict[datetime, SingleExpirySurfaceSliceProgressRow],
    completed_valuation_count: int,
    active_count: int,
    valuation_count: int,
    surface_parallelism: int,
    total_cpu_count: int,
    pricing_cpu_count: int,
    detail: Optional[str] = None,
) -> None:
    progress.update(
        detail=detail,
        completed_units=completed_valuation_count,
        total_units=valuation_count,
        subdetail=build_single_expiry_surface_progress_panel(
            rows_by_time=rows_by_time,
            active_count=active_count,
            surface_parallelism=surface_parallelism,
            total_cpu_count=total_cpu_count,
            pricing_cpu_count=pricing_cpu_count,
        ),
    )


def _retained_snapshot_valuation_count(
    *,
    prepared_snapshot_groups: tuple[PreparedSnapshotGroupInput, ...],
) -> int:
    return len(
        {
            node.quote_timestamp_utc
            for prepared_group in prepared_snapshot_groups
            for node in prepared_group.nodes
        }
    )


def prepare_single_expiry_surface_slice_input(
    *,
    context: PreparedObservedOptionMarketContext,
    panel_valuation_time_utc: datetime,
    prepared_snapshot_groups: tuple[PreparedSnapshotGroupInput, ...],
) -> PreparedSingleExpirySurfaceSliceInput:
    filtered_tick_data = filter_observed_tick_data(
        tick_data_frame=context.tick_data_frame,
        timestamp_column=context.resolved_tick_column_mapping.timestamp_column,
        valuation_time_utc=panel_valuation_time_utc,
    )

    spot = observed_spot_from_tick_data(
        filtered_tick_data,
        price_column=context.resolved_tick_column_mapping.price_column,
    )

    return PreparedSingleExpirySurfaceSliceInput(
        valuation_time_utc=panel_valuation_time_utc,
        filtered_tick_data=filtered_tick_data,
        spot=spot,
        prepared_snapshot_groups=prepared_snapshot_groups,
    )


def _price_single_expiry_surface_slice(
    *,
    model: JaniformModel,
    resolved_tick_column_mapping: CalibrationTickDataColumnMapping,
    slice_input: PreparedSingleExpirySurfaceSliceInput,
    risk_free_rate: float,
    dividend_yield: float,
    cpu_count: int,
    silent: bool,
    progress_mode: WorkflowProgressMode | str,
    progress_queue: Optional[Any] = None,
) -> JaniformObservedOptionSnapshotPricingResult:
    progress = (
        None
        if progress_queue is None
        else SingleExpirySurfaceSliceWorkerProgress(
            valuation_time_utc=slice_input.valuation_time_utc,
            progress_queue=progress_queue,
        )
    )
    if progress is not None:
        progress.notify_started()

    return price_prepared_snapshot_groups_for_model(
        model,
        valuation_time_utc=slice_input.valuation_time_utc,
        spot=slice_input.spot,
        prepared_snapshot_groups=slice_input.prepared_snapshot_groups,
        filtered_tick_data=slice_input.filtered_tick_data,
        resolved_tick_column_mapping=resolved_tick_column_mapping,
        risk_free_rate=risk_free_rate,
        dividend_yield=dividend_yield,
        cpu_count=cpu_count,
        silent=silent,
        progress_mode=progress_mode,
        progress=progress,
    )


__all__ = [
    "build_observed_option_surface_pricing_result",
    "price_option_surface_for_model",
    "resolve_single_expiry_surface_valuation_times",
]
