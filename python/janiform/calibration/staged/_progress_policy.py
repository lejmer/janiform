from dataclasses import dataclass
from typing import Optional


# Internal UI-facing weighting policy for staged calibration live progress.
# These values are intentionally centralized here so we can easily tune them
# without changing individual frontends.
_CALIBRATION_TOTAL_PROGRESS_STAGE_WEIGHT_POLICY_V1: dict[str, float] = {
    "seasonality": 0.02,
    "hawkes": 0.01,
    "return_components": 0.04,
    "mean_reversion": 0.01,
    "joint_refinement": 0.92,
}

_CALIBRATION_TOTAL_PROGRESS_COMPLETION_RESERVE_V1: float = 0.01

_CALIBRATION_JOINT_REFINEMENT_PROGRESS_PHASE_WEIGHT_POLICY_V1: dict[str, float] = {
    "restart_optimization": 0.90,
    "coarse_consistency_evaluation": 0.05,
    "final_seed_evaluation": 0.03,
    "final_assembly": 0.02,
}


@dataclass(frozen=True, slots=True)
class CalibrationLiveProgressSnapshot:
    status: str
    stage_sequence: tuple[str, ...] = ()
    stage_index: Optional[int] = None
    stage_count: Optional[int] = None
    current_stage: Optional[str] = None
    current_subtask: Optional[str] = None
    current_stage_is_complete: bool = False
    subtask_completed_units: Optional[int] = None
    subtask_total_units: Optional[int] = None


def calibration_total_progress_fraction(
    snapshot: CalibrationLiveProgressSnapshot,
) -> Optional[float]:
    if snapshot.status == "completed":
        return 1.0

    stage_index = snapshot.stage_index
    if not isinstance(stage_index, int) or stage_index <= 0:
        return None

    stage_weights = _stage_weights_for_run_stage_sequence(snapshot.stage_sequence)
    if stage_weights is not None and stage_index <= len(stage_weights):
        completed_weight = sum(stage_weights[: max(stage_index - 1, 0)])
        if snapshot.current_stage_is_complete:
            stage_execution_fraction = min(
                max(completed_weight + stage_weights[stage_index - 1], 0.0),
                1.0,
            )
            return (
                1.0 - _CALIBRATION_TOTAL_PROGRESS_COMPLETION_RESERVE_V1
            ) * stage_execution_fraction

        current_stage_fraction = _calibration_current_stage_progress_fraction(snapshot)

        stage_execution_fraction = min(
            max(completed_weight + current_stage_fraction * stage_weights[stage_index - 1], 0.0),
            1.0,
        )

        return (
            1.0 - _CALIBRATION_TOTAL_PROGRESS_COMPLETION_RESERVE_V1
        ) * stage_execution_fraction

    stage_count = snapshot.stage_count
    if not isinstance(stage_count, int) or stage_count <= 0:
        return None

    if snapshot.current_stage_is_complete:
        return (1.0 - _CALIBRATION_TOTAL_PROGRESS_COMPLETION_RESERVE_V1) * min(
            max(stage_index / stage_count, 0.0),
            1.0,
        )

    completed_stage_count = max(stage_index - 1, 0)

    current_stage_fraction = _calibration_current_stage_progress_fraction(snapshot)

    return (1.0 - _CALIBRATION_TOTAL_PROGRESS_COMPLETION_RESERVE_V1) * min(
        max((completed_stage_count + current_stage_fraction) / stage_count, 0.0),
        1.0,
    )


def _calibration_current_stage_progress_fraction(
    snapshot: CalibrationLiveProgressSnapshot,
) -> float:
    joint_refinement_phase_progress_fraction = _joint_refinement_phase_progress_fraction(
        snapshot
    )
    if joint_refinement_phase_progress_fraction is not None:
        return joint_refinement_phase_progress_fraction

    subtask_progress_fraction = _progress_bar_fraction(
        snapshot.subtask_completed_units,
        snapshot.subtask_total_units,
        is_complete=snapshot.current_stage_is_complete,
    )
    if subtask_progress_fraction is not None:
        return subtask_progress_fraction

    return 1.0 if snapshot.current_stage_is_complete else 0.0


def progress_bar_display_units(
    completed_units: Optional[int],
    total_units: Optional[int],
    *,
    is_complete: bool = False,
) -> Optional[tuple[int, int]]:
    if (
        not isinstance(completed_units, int)
        or not isinstance(total_units, int)
        or total_units <= 0
    ):
        return None

    if is_complete:
        return min(max(completed_units, 0), total_units), total_units

    display_completed_units = min(
        max(completed_units - 1, 0),
        total_units - 1,
    )

    return display_completed_units, total_units


def _progress_bar_fraction(
    completed_units: Optional[int],
    total_units: Optional[int],
    *,
    is_complete: bool = False,
) -> Optional[float]:
    display_units = progress_bar_display_units(
        completed_units,
        total_units,
        is_complete=is_complete,
    )
    if display_units is None:
        return None

    display_completed_units, display_total_units = display_units

    return min(
        max(display_completed_units / display_total_units, 0.0),
        1.0,
    )


def _joint_refinement_phase_progress_fraction(
    snapshot: CalibrationLiveProgressSnapshot,
) -> Optional[float]:
    if snapshot.current_stage != "joint_refinement":
        return None

    if snapshot.current_stage_is_complete:
        return 1.0

    current_subtask = snapshot.current_subtask
    if not isinstance(current_subtask, str) or not current_subtask.startswith(
        "joint_refinement."
    ):
        return 0.0

    progress_phase = current_subtask.split(".", 1)[1]
    phase_weights = _joint_refinement_phase_weights()
    if phase_weights is None or progress_phase not in phase_weights:
        return None

    phase_names = tuple(phase_weights.keys())
    phase_index = phase_names.index(progress_phase)

    current_phase_fraction = _progress_bar_fraction(
        snapshot.subtask_completed_units,
        snapshot.subtask_total_units,
    )
    if current_phase_fraction is None:
        current_phase_fraction = 0.0

    completed_phase_weight = sum(
        phase_weights[phase_name] for phase_name in phase_names[:phase_index]
    )

    return completed_phase_weight + current_phase_fraction * phase_weights[progress_phase]


def _joint_refinement_phase_weights() -> Optional[dict[str, float]]:
    total_weight = sum(_CALIBRATION_JOINT_REFINEMENT_PROGRESS_PHASE_WEIGHT_POLICY_V1.values())
    if total_weight <= 0.0:
        return None

    return {
        phase_name: phase_weight / total_weight
        for phase_name, phase_weight in _CALIBRATION_JOINT_REFINEMENT_PROGRESS_PHASE_WEIGHT_POLICY_V1.items()
    }


def _stage_weights_for_run_stage_sequence(
    stage_sequence: tuple[str, ...],
) -> Optional[tuple[float, ...]]:
    if len(stage_sequence) == 0:
        return None

    raw_stage_weights = [
        _CALIBRATION_TOTAL_PROGRESS_STAGE_WEIGHT_POLICY_V1.get(stage_name)
        for stage_name in stage_sequence
    ]
    if any(weight is None for weight in raw_stage_weights):
        return None

    total_weight = sum(weight for weight in raw_stage_weights if weight is not None)
    if total_weight <= 0.0:
        return None

    return tuple(
        weight / total_weight
        for weight in raw_stage_weights
        if weight is not None
    )
