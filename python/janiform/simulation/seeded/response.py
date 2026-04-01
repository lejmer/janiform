from __future__ import annotations

from collections.abc import Callable, Iterator, Sequence
from dataclasses import dataclass
from time import perf_counter
from typing import Any, Optional, cast

from .contracts import (
    SEEDED_TICK_SIMULATION_BOUNDARY_TRANSPORT_POLICY,
    SEEDED_TICK_SIMULATION_DIAGNOSTICS_SCHEMA_VERSION,
    SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
    SeededFinePathMetadata,
    SeededFinePathNewsArrivalComponent,
    SeededFinePathRecord,
    SeededFinePathTransitionKind,
    SeededTickEventRecord,
    SeededTickEventSide,
    SeededTickEventStateSnapshot,
    SeededTickEventTapeMetadata,
    SeededTickSimulationDiagnostics,
    _project_event_tape_from_fine_path,
)
from ..shared.errors import (
    SIMULATION_ERROR_FACTORY,
    SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
    _simulation_error,
    _simulation_integration_error,
)
from ...contracts import (
    ContractModel,
    parse_json_object,
    require_mapping,
    require_non_negative_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)
from ...calendar import MarketState
from ...risk import SufficientStatisticsRiskReturnComponents


class _SeededTickSimulationMaterializationTracker:
    __slots__ = ("_diagnostics",)

    def __init__(self, diagnostics: SeededTickSimulationDiagnostics) -> None:
        self._diagnostics = diagnostics

    def record_fine_path_chunk(
        self,
        *,
        record_count: int,
        boundary_transport_bytes: int,
        python_materialization_seconds: float,
    ) -> None:
        self._diagnostics.boundary_transport_bytes += boundary_transport_bytes
        self._diagnostics.python_materialization_seconds += python_materialization_seconds
        self._diagnostics.materialized_transition_count += record_count

    def record_event_tape_chunk(
        self,
        *,
        record_count: int,
        boundary_transport_bytes: int,
        python_materialization_seconds: float,
    ) -> None:
        self._diagnostics.boundary_transport_bytes += boundary_transport_bytes
        self._diagnostics.python_materialization_seconds += python_materialization_seconds
        self._diagnostics.materialized_event_count += record_count


type _NativeChunkLoader = Callable[[int, int], Any]
type _NativeChunkParser[T] = Callable[[Any, int], tuple[T, ...]]
type _ChunkTransportEstimator = Callable[[Any], int]
type _ChunkTrackerCallback = Callable[[int, int, float], None]


class _NativeBackedSequenceView[T](Sequence[T]):
    __slots__ = (
        "_cached_chunk_records",
        "_cached_chunk_start",
        "_chunk_record_count",
        "_chunk_tracker",
        "_field_path",
        "_index_error_message",
        "_materialized_records",
        "_native_chunk_loader",
        "_native_chunk_parser",
        "_chunk_transport_estimator",
        "_record_count",
    )

    def __init__(
        self,
        *,
        field_path: str,
        index_error_message: str,
        native_chunk_parser: _NativeChunkParser[T],
        materialized_records: Optional[tuple[T, ...]] = None,
        native_chunk_loader: Optional[_NativeChunkLoader] = None,
        record_count: Optional[int] = None,
        chunk_record_count: int = SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
        chunk_transport_estimator: Optional[_ChunkTransportEstimator] = None,
        chunk_tracker: Optional[_ChunkTrackerCallback] = None,
    ) -> None:
        if materialized_records is None and native_chunk_loader is None:
            raise _simulation_error(
                field_path,
                "must be backed by materialized records or a native result handle",
            )

        self._materialized_records = materialized_records
        self._native_chunk_loader = native_chunk_loader
        self._record_count = len(materialized_records) if materialized_records is not None else record_count
        self._chunk_record_count = chunk_record_count
        self._field_path = field_path
        self._index_error_message = index_error_message
        self._native_chunk_parser = native_chunk_parser
        self._chunk_transport_estimator = chunk_transport_estimator
        self._chunk_tracker = chunk_tracker
        self._cached_chunk_start: Optional[int] = None
        self._cached_chunk_records: tuple[T, ...] = ()

    def __eq__(self, other: object) -> bool:
        if isinstance(other, Sequence) and not isinstance(other, str):
            return tuple(self) == tuple(other)

        return False

    def __len__(self) -> int:
        if self._materialized_records is not None:
            return len(self._materialized_records)

        return cast(int, self._record_count)

    def __getitem__(self, index: int | slice) -> T | tuple[T, ...]:
        if isinstance(index, slice):
            start_index, stop_index, step = index.indices(len(self))
            return tuple(self[position] for position in range(start_index, stop_index, step))

        normalized_index = self._normalize_index(index)
        if self._materialized_records is not None:
            return self._materialized_records[normalized_index]

        if (
            self._cached_chunk_start is not None
            and normalized_index >= self._cached_chunk_start
            and normalized_index < self._cached_chunk_start + len(self._cached_chunk_records)
        ):
            return self._cached_chunk_records[normalized_index - self._cached_chunk_start]

        chunk_start = (normalized_index // self._chunk_record_count) * self._chunk_record_count
        self._cached_chunk_records = self._load_native_chunk(chunk_start)
        self._cached_chunk_start = chunk_start
        return self._cached_chunk_records[normalized_index - chunk_start]

    def __iter__(self) -> Iterator[T]:
        if self._materialized_records is not None:
            return iter(self._materialized_records)

        return self._iter_native_records()

    def _iter_native_records(self) -> Iterator[T]:
        record_count = len(self)
        next_record_position = 0
        while next_record_position < record_count:
            for record in self._load_native_chunk(next_record_position):
                yield record

            next_record_position += self._chunk_record_count

    def _load_native_chunk(self, start_index: int) -> tuple[T, ...]:
        try:
            chunk_payload = cast(_NativeChunkLoader, self._native_chunk_loader)(
                start_index,
                self._chunk_record_count,
            )
        except Exception as e:
            raise _simulation_integration_error(
                f"native {self._field_path} chunk projection failed",
                entrypoint="run_seeded_tick_event_simulation",
                error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
                field_path=self._field_path,
                context={"chunk_start_index": start_index},
            ) from e

        try:
            parse_started = perf_counter()
            chunk_records = self._native_chunk_parser(chunk_payload, start_index)
            parse_seconds = perf_counter() - parse_started
        except Exception as e:
            raise _simulation_integration_error(
                f"native {self._field_path} chunk materialization failed",
                entrypoint="run_seeded_tick_event_simulation",
                error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
                field_path=self._field_path,
                context={"chunk_start_index": start_index},
            ) from e

        if self._chunk_tracker is not None:
            self._chunk_tracker(
                record_count=len(chunk_records),
                boundary_transport_bytes=(
                    self._chunk_transport_estimator(chunk_payload)
                    if self._chunk_transport_estimator is not None
                    else 0
                ),
                python_materialization_seconds=parse_seconds,
            )

        return chunk_records

    def _normalize_index(self, index: int) -> int:
        normalized_index = index
        if normalized_index < 0:
            normalized_index += len(self)
        if normalized_index < 0 or normalized_index >= len(self):
            raise IndexError(self._index_error_message)

        return normalized_index


def _parse_seeded_fine_path_view_record(
    raw_record: dict[str, Any],
    record_index: int,
) -> SeededFinePathRecord:
    return SeededFinePathRecord.from_mapping(
        raw_record,
        field_path=f"seeded_tick_simulation_response.fine_path[{record_index}]",
    )


def _is_public_seeded_fine_path_record(record: SeededFinePathRecord) -> bool:
    return not (
        record.transition_kind == SeededFinePathTransitionKind.EXOGENOUS_NEWS
        and record.market_state.value == "closed"
    )


def _parse_seeded_event_tape_view_record(
    raw_record: dict[str, Any],
    record_index: int,
) -> SeededTickEventRecord:
    return SeededTickEventRecord.from_mapping(
        raw_record,
        field_path=f"seeded_tick_simulation_response.event_tape[{record_index}]",
    )


def _estimate_typed_chunk_boundary_transport_bytes(chunk_payload: Any) -> int:
    estimated_transport_bytes = getattr(chunk_payload, "estimated_transport_bytes", None)
    if isinstance(estimated_transport_bytes, int):
        return estimated_transport_bytes

    return 0


def _build_state_snapshot(
    *,
    y_plus_plus: Optional[float],
    y_plus_minus: Optional[float],
    y_minus_plus: Optional[float],
    y_minus_minus: Optional[float],
    deseasonalized_lambda_plus: Optional[float],
    deseasonalized_lambda_minus: Optional[float],
) -> Optional[SeededTickEventStateSnapshot]:
    if (
        y_plus_plus is None
        and y_plus_minus is None
        and y_minus_plus is None
        and y_minus_minus is None
        and deseasonalized_lambda_plus is None
        and deseasonalized_lambda_minus is None
    ):
        return None

    return SeededTickEventStateSnapshot(
        y_plus_plus=cast(float, y_plus_plus),
        y_plus_minus=cast(float, y_plus_minus),
        y_minus_plus=cast(float, y_minus_plus),
        y_minus_minus=cast(float, y_minus_minus),
        deseasonalized_lambda_plus=cast(float, deseasonalized_lambda_plus),
        deseasonalized_lambda_minus=cast(float, deseasonalized_lambda_minus),
    )


def _parse_seeded_fine_path_chunk_columns(
    chunk_payload: Any,
    start_index: int,
) -> tuple[SeededFinePathRecord, ...]:
    transition_index_column = cast(list[int], chunk_payload.transition_index)
    timestamp_column = cast(list[int], chunk_payload.timestamp_utc_nanos)
    transition_kind_column = cast(list[str], chunk_payload.transition_kind)
    side_column = cast(list[Optional[str]], chunk_payload.side)
    news_arrival_component_column = cast(
        list[Optional[str]],
        chunk_payload.news_arrival_component,
    )
    market_state_column = cast(list[str], chunk_payload.market_state)
    buy_side_return_column = cast(list[float], chunk_payload.buy_side_return)
    sell_side_return_column = cast(list[float], chunk_payload.sell_side_return)
    trading_time_return_column = cast(list[float], chunk_payload.trading_time_return)
    open_jump_return_column = cast(list[float], chunk_payload.open_jump_return)
    closed_gap_return_column = cast(list[float], chunk_payload.closed_gap_return)
    total_return_column = cast(list[float], chunk_payload.total_return)
    core_total_return_increment_column = cast(
        list[float],
        chunk_payload.core_total_return_increment,
    )
    observed_log_price_column = cast(list[float], chunk_payload.observed_log_price)
    anchor_value_column = cast(list[float], chunk_payload.anchor_value)
    deviation_from_anchor_column = cast(
        list[float],
        chunk_payload.deviation_from_anchor,
    )
    cumulative_core_log_return_column = cast(
        list[float],
        chunk_payload.cumulative_core_log_return,
    )
    state_snapshot_y_plus_plus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_y_plus_plus,
    )
    state_snapshot_y_plus_minus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_y_plus_minus,
    )
    state_snapshot_y_minus_plus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_y_minus_plus,
    )
    state_snapshot_y_minus_minus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_y_minus_minus,
    )
    state_snapshot_deseasonalized_lambda_plus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_deseasonalized_lambda_plus,
    )
    state_snapshot_deseasonalized_lambda_minus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_deseasonalized_lambda_minus,
    )

    _validate_native_chunk_column_lengths(
        "fine_path",
        start_index,
        len(transition_index_column),
        timestamp_column,
        transition_kind_column,
        side_column,
        news_arrival_component_column,
        market_state_column,
        buy_side_return_column,
        sell_side_return_column,
        trading_time_return_column,
        open_jump_return_column,
        closed_gap_return_column,
        total_return_column,
        core_total_return_increment_column,
        observed_log_price_column,
        anchor_value_column,
        deviation_from_anchor_column,
        cumulative_core_log_return_column,
        state_snapshot_y_plus_plus_column,
        state_snapshot_y_plus_minus_column,
        state_snapshot_y_minus_plus_column,
        state_snapshot_y_minus_minus_column,
        state_snapshot_deseasonalized_lambda_plus_column,
        state_snapshot_deseasonalized_lambda_minus_column,
    )

    return tuple(
        SeededFinePathRecord(
            transition_index=transition_index_column[chunk_offset],
            timestamp_utc_nanos=timestamp_column[chunk_offset],
            transition_kind=SeededFinePathTransitionKind(
                transition_kind_column[chunk_offset]
            ),
            side=(
                SeededTickEventSide(side_column[chunk_offset])
                if side_column[chunk_offset] is not None
                else None
            ),
            news_arrival_component=(
                SeededFinePathNewsArrivalComponent(
                    news_arrival_component_column[chunk_offset]
                )
                if news_arrival_component_column[chunk_offset] is not None
                else None
            ),
            market_state=MarketState(market_state_column[chunk_offset]),
            return_components=SufficientStatisticsRiskReturnComponents(
                buy_side_return=buy_side_return_column[chunk_offset],
                sell_side_return=sell_side_return_column[chunk_offset],
                trading_time_return=trading_time_return_column[chunk_offset],
                open_jump_return=open_jump_return_column[chunk_offset],
                closed_gap_return=closed_gap_return_column[chunk_offset],
                total_return=total_return_column[chunk_offset],
            ),
            core_total_return_increment=core_total_return_increment_column[chunk_offset],
            observed_log_price=observed_log_price_column[chunk_offset],
            anchor_value=anchor_value_column[chunk_offset],
            deviation_from_anchor=deviation_from_anchor_column[chunk_offset],
            cumulative_core_log_return=cumulative_core_log_return_column[chunk_offset],
            state_snapshot=_build_state_snapshot(
                y_plus_plus=state_snapshot_y_plus_plus_column[chunk_offset],
                y_plus_minus=state_snapshot_y_plus_minus_column[chunk_offset],
                y_minus_plus=state_snapshot_y_minus_plus_column[chunk_offset],
                y_minus_minus=state_snapshot_y_minus_minus_column[chunk_offset],
                deseasonalized_lambda_plus=(
                    state_snapshot_deseasonalized_lambda_plus_column[chunk_offset]
                ),
                deseasonalized_lambda_minus=(
                    state_snapshot_deseasonalized_lambda_minus_column[chunk_offset]
                ),
            ),
        )
        for chunk_offset in range(len(transition_index_column))
    )


def _parse_seeded_event_tape_chunk_columns(
    chunk_payload: Any,
    start_index: int,
) -> tuple[SeededTickEventRecord, ...]:
    event_index_column = cast(list[int], chunk_payload.event_index)
    timestamp_column = cast(list[int], chunk_payload.timestamp_utc_nanos)
    side_column = cast(list[str], chunk_payload.side)
    core_total_return_increment_column = cast(
        list[float],
        chunk_payload.core_total_return_increment,
    )
    observed_log_price_column = cast(list[float], chunk_payload.observed_log_price)
    anchor_value_column = cast(list[float], chunk_payload.anchor_value)
    deviation_from_anchor_column = cast(
        list[float],
        chunk_payload.deviation_from_anchor,
    )
    state_snapshot_y_plus_plus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_y_plus_plus,
    )
    state_snapshot_y_plus_minus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_y_plus_minus,
    )
    state_snapshot_y_minus_plus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_y_minus_plus,
    )
    state_snapshot_y_minus_minus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_y_minus_minus,
    )
    state_snapshot_deseasonalized_lambda_plus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_deseasonalized_lambda_plus,
    )
    state_snapshot_deseasonalized_lambda_minus_column = cast(
        list[Optional[float]],
        chunk_payload.state_snapshot_deseasonalized_lambda_minus,
    )

    _validate_native_chunk_column_lengths(
        "event_tape",
        start_index,
        len(event_index_column),
        timestamp_column,
        side_column,
        core_total_return_increment_column,
        observed_log_price_column,
        anchor_value_column,
        deviation_from_anchor_column,
        state_snapshot_y_plus_plus_column,
        state_snapshot_y_plus_minus_column,
        state_snapshot_y_minus_plus_column,
        state_snapshot_y_minus_minus_column,
        state_snapshot_deseasonalized_lambda_plus_column,
        state_snapshot_deseasonalized_lambda_minus_column,
    )

    return tuple(
        SeededTickEventRecord(
            event_index=event_index_column[chunk_offset],
            timestamp_utc_nanos=timestamp_column[chunk_offset],
            side=SeededTickEventSide(side_column[chunk_offset]),
            core_total_return_increment=core_total_return_increment_column[chunk_offset],
            observed_log_price=observed_log_price_column[chunk_offset],
            anchor_value=anchor_value_column[chunk_offset],
            deviation_from_anchor=deviation_from_anchor_column[chunk_offset],
            state_snapshot=_build_state_snapshot(
                y_plus_plus=state_snapshot_y_plus_plus_column[chunk_offset],
                y_plus_minus=state_snapshot_y_plus_minus_column[chunk_offset],
                y_minus_plus=state_snapshot_y_minus_plus_column[chunk_offset],
                y_minus_minus=state_snapshot_y_minus_minus_column[chunk_offset],
                deseasonalized_lambda_plus=(
                    state_snapshot_deseasonalized_lambda_plus_column[chunk_offset]
                ),
                deseasonalized_lambda_minus=(
                    state_snapshot_deseasonalized_lambda_minus_column[chunk_offset]
                ),
            ),
        )
        for chunk_offset in range(len(event_index_column))
    )


def _validate_native_chunk_column_lengths(
    field_path: str,
    start_index: int,
    expected_length: int,
    *columns: list[Any],
) -> None:
    for column_index, column in enumerate(columns, start=1):
        if len(column) != expected_length:
            raise _simulation_integration_error(
                f"native {field_path} chunk columns had inconsistent lengths",
                entrypoint="run_seeded_tick_event_simulation",
                error_id=SIMULATION_INTEGRATION_RESPONSE_ERROR_ID,
                field_path=field_path,
                context={
                    "chunk_start_index": start_index,
                    "expected_length": expected_length,
                    "column_index": column_index,
                    "actual_length": len(column),
                },
            )


def _require_native_result_attribute(
    native_result: Any,
    attribute_name: str,
    *,
    field_path: str,
) -> Any:
    try:
        return getattr(native_result, attribute_name)
    except AttributeError as e:
        raise _simulation_error(
            field_path,
            f"native result handle is missing required attribute `{attribute_name}`",
        ) from e


def _require_native_result_json_method(
    native_result: Any,
    method_name: str,
    *,
    field_path: str,
) -> str:
    raw_method = _require_native_result_attribute(
        native_result,
        method_name,
        field_path=field_path,
    )
    if not callable(raw_method):
        raise _simulation_error(
            field_path,
            f"native result attribute `{method_name}` must be callable",
        )

    try:
        payload = raw_method()
    except Exception as e:
        raise _simulation_error(
            field_path,
            f"native result method `{method_name}` failed while building the response",
        ) from e

    if not isinstance(payload, str):
        raise _simulation_error(
            field_path,
            f"native result method `{method_name}` must return a JSON string",
        )

    return payload


class SeededFinePathView(_NativeBackedSequenceView[SeededFinePathRecord]):
    __slots__ = ()

    def __init__(
        self,
        *,
        materialized_records: Optional[tuple[SeededFinePathRecord, ...]] = None,
        native_result: Optional[Any] = None,
        chunk_record_count: int = SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
        tracker: Optional[_SeededTickSimulationMaterializationTracker] = None,
    ) -> None:
        super().__init__(
            field_path="fine_path",
            index_error_message="seeded fine path index out of range",
            native_chunk_parser=_parse_seeded_fine_path_chunk_columns,
            materialized_records=materialized_records,
            native_chunk_loader=(
                native_result.get_fine_path_chunk_columns if native_result is not None else None
            ),
            record_count=(
                int(native_result.transition_count) if native_result is not None else None
            ),
            chunk_record_count=chunk_record_count,
            chunk_transport_estimator=_estimate_typed_chunk_boundary_transport_bytes,
            chunk_tracker=(
                tracker.record_fine_path_chunk if tracker is not None else None
            ),
        )


class SeededTickEventTapeView(_NativeBackedSequenceView[SeededTickEventRecord]):
    __slots__ = ()

    def __init__(
        self,
        *,
        materialized_records: Optional[tuple[SeededTickEventRecord, ...]] = None,
        native_result: Optional[Any] = None,
        chunk_record_count: int = SEEDED_TICK_SIMULATION_NATIVE_CHUNK_RECORD_COUNT,
        tracker: Optional[_SeededTickSimulationMaterializationTracker] = None,
    ) -> None:
        super().__init__(
            field_path="event_tape",
            index_error_message="seeded event tape index out of range",
            native_chunk_parser=_parse_seeded_event_tape_chunk_columns,
            materialized_records=materialized_records,
            native_chunk_loader=(
                native_result.get_event_tape_chunk_columns if native_result is not None else None
            ),
            record_count=int(native_result.event_count) if native_result is not None else None,
            chunk_record_count=chunk_record_count,
            chunk_transport_estimator=_estimate_typed_chunk_boundary_transport_bytes,
            chunk_tracker=(
                tracker.record_event_tape_chunk if tracker is not None else None
            ),
        )


@dataclass
class SeededTickSimulationResponse(ContractModel):
    """Typed seeded-tick simulation response containing lazy fine-path and event-tape views."""

    schema_version: str
    metadata: SeededFinePathMetadata
    event_tape_metadata: SeededTickEventTapeMetadata
    diagnostics: SeededTickSimulationDiagnostics
    fine_path: SeededFinePathView
    event_tape: SeededTickEventTapeView

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "seeded_tick_simulation_response",
    ) -> SeededTickSimulationResponse:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        fine_path_items = require_sequence(
            raw_payload,
            "fine_path",
            f"{field_path}.fine_path",
            SIMULATION_ERROR_FACTORY,
        )
        fine_path_records = tuple(
            SeededFinePathRecord.from_mapping(
                raw_item,
                field_path=f"{field_path}.fine_path[{transition_index}]",
            )
            for transition_index, raw_item in enumerate(fine_path_items)
        )

        event_tape_items = require_sequence(
            raw_payload,
            "event_tape",
            f"{field_path}.event_tape",
            SIMULATION_ERROR_FACTORY,
        )
        event_tape_records = tuple(
            SeededTickEventRecord.from_mapping(
                raw_item,
                field_path=f"{field_path}.event_tape[{event_index}]",
            )
            for event_index, raw_item in enumerate(event_tape_items)
        )
        projected_event_tape = _project_event_tape_from_fine_path(fine_path_records)
        if event_tape_records != projected_event_tape:
            raise _simulation_error(
                f"{field_path}.event_tape",
                "must match the deterministic event projection derived from fine_path",
            )

        diagnostics = SeededTickSimulationDiagnostics.from_mapping(
            require_mapping(
                raw_payload,
                "diagnostics",
                f"{field_path}.diagnostics",
                SIMULATION_ERROR_FACTORY,
            ),
            field_path=f"{field_path}.diagnostics",
        )

        return SeededTickSimulationResponse(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            metadata=SeededFinePathMetadata.from_mapping(
                require_mapping(
                    raw_payload,
                    "metadata",
                    f"{field_path}.metadata",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.metadata",
            ),
            event_tape_metadata=SeededTickEventTapeMetadata.from_mapping(
                require_mapping(
                    raw_payload,
                    "event_tape_metadata",
                    f"{field_path}.event_tape_metadata",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.event_tape_metadata",
            ),
            diagnostics=diagnostics,
            fine_path=SeededFinePathView(materialized_records=fine_path_records),
            event_tape=SeededTickEventTapeView(materialized_records=event_tape_records),
        )

    @staticmethod
    def from_native_result(
        *,
        native_result: Any,
        native_runtime_seconds: float,
        python_response_construction_seconds: float,
        chunk_record_count: int,
    ) -> SeededTickSimulationResponse:
        schema_version = require_string(
            {
                "schema_version": _require_native_result_attribute(
                    native_result,
                    "schema_version",
                    field_path="seeded_tick_simulation_response.schema_version",
                )
            },
            "schema_version",
            "seeded_tick_simulation_response.schema_version",
            SIMULATION_ERROR_FACTORY,
        )
        transition_count = require_non_negative_integer(
            {
                "transition_count": _require_native_result_attribute(
                    native_result,
                    "transition_count",
                    field_path="seeded_tick_simulation_response.diagnostics.transition_count",
                )
            },
            "transition_count",
            "seeded_tick_simulation_response.diagnostics.transition_count",
            SIMULATION_ERROR_FACTORY,
        )
        event_count = require_non_negative_integer(
            {
                "event_count": _require_native_result_attribute(
                    native_result,
                    "event_count",
                    field_path="seeded_tick_simulation_response.diagnostics.event_count",
                )
            },
            "event_count",
            "seeded_tick_simulation_response.diagnostics.event_count",
            SIMULATION_ERROR_FACTORY,
        )
        native_estimated_retained_bytes = require_non_negative_integer(
            {
                "estimated_native_retained_bytes": _require_native_result_attribute(
                    native_result,
                    "estimated_native_retained_bytes",
                    field_path=(
                        "seeded_tick_simulation_response.diagnostics.native_estimated_retained_bytes"
                    ),
                )
            },
            "estimated_native_retained_bytes",
            "seeded_tick_simulation_response.diagnostics.native_estimated_retained_bytes",
            SIMULATION_ERROR_FACTORY,
        )
        diagnostics = SeededTickSimulationDiagnostics(
            schema_version=SEEDED_TICK_SIMULATION_DIAGNOSTICS_SCHEMA_VERSION,
            transition_count=transition_count,
            event_count=event_count,
            native_runtime_seconds=native_runtime_seconds,
            python_response_construction_seconds=python_response_construction_seconds,
            python_materialization_seconds=0.0,
            boundary_transport_bytes=0,
            native_estimated_retained_bytes=native_estimated_retained_bytes,
            materialized_transition_count=0,
            materialized_event_count=0,
            boundary_transport_policy=SEEDED_TICK_SIMULATION_BOUNDARY_TRANSPORT_POLICY,
            boundary_chunk_record_count=chunk_record_count,
        )
        tracker = _SeededTickSimulationMaterializationTracker(diagnostics)
        metadata_payload = parse_json_object(
            _require_native_result_json_method(
                native_result,
                "fine_path_metadata_json",
                field_path="seeded_tick_simulation_response.metadata",
            ),
            "seeded fine-path metadata",
            SIMULATION_ERROR_FACTORY,
        )
        event_tape_metadata_payload = parse_json_object(
            _require_native_result_json_method(
                native_result,
                "event_tape_metadata_json",
                field_path="seeded_tick_simulation_response.event_tape_metadata",
            ),
            "seeded event-tape metadata",
            SIMULATION_ERROR_FACTORY,
        )

        response = SeededTickSimulationResponse(
            schema_version=schema_version,
            metadata=SeededFinePathMetadata.from_mapping(
                dict(metadata_payload),
                field_path="seeded_tick_simulation_response.metadata",
            ),
            event_tape_metadata=SeededTickEventTapeMetadata.from_mapping(
                dict(event_tape_metadata_payload),
                field_path="seeded_tick_simulation_response.event_tape_metadata",
            ),
            diagnostics=diagnostics,
            fine_path=SeededFinePathView(
                native_result=native_result,
                chunk_record_count=chunk_record_count,
                tracker=tracker,
            ),
            event_tape=SeededTickEventTapeView(
                native_result=native_result,
                chunk_record_count=chunk_record_count,
                tracker=tracker,
            ),
        )

        response._native_result = native_result
        return response

def validate_seeded_tick_simulation_response(
    raw_response: dict[str, Any] | Any,
) -> SeededTickSimulationResponse:
    return SeededTickSimulationResponse.from_mapping(raw_response)


def serialize_seeded_tick_simulation_response(response: SeededTickSimulationResponse) -> str:
    return serialize_deterministic_json(response)


def deserialize_seeded_tick_simulation_response(payload_json: str) -> SeededTickSimulationResponse:
    raw_payload = parse_json_object(payload_json, "seeded tick simulation response", SIMULATION_ERROR_FACTORY)
    return validate_seeded_tick_simulation_response(dict(raw_payload))


__all__ = [
    "SeededFinePathView",
    "SeededTickEventTapeView",
    "SeededTickSimulationResponse",
    "deserialize_seeded_tick_simulation_response",
    "serialize_seeded_tick_simulation_response",
    "validate_seeded_tick_simulation_response",
]
