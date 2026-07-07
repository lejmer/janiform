from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Optional

from ..errors import calibration_execution_error
from ..staged.results import build_staged_failure_context, format_staged_failure_summary
from .report import JaniformFitReport

if TYPE_CHECKING:
    from ...model import JaniformModel


@dataclass(frozen=True, slots=True)
class JaniformFitResult:
    model: Optional[JaniformModel]
    report: JaniformFitReport

    @property
    def succeeded(self) -> bool:
        return self.report.success

    def require_model(self) -> JaniformModel:
        if self.model is not None:
            return self.model

        raise calibration_execution_error(
            format_staged_failure_summary(self.report.staged_response),
            entrypoint="fit_with_report",
            context=build_staged_failure_context(self.report.staged_response),
        )


__all__ = ["JaniformFitResult"]
