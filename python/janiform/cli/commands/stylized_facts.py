from argparse import Namespace
from pathlib import Path
from typing import Any, Optional, cast

from .._constants import (
    CLI_STYLIZED_FACTS_HORIZON_COMPARISON_ENTRYPOINT,
    CLI_STYLIZED_FACTS_HORIZON_COMPARISON_INTERNAL_ERROR_ID,
    CLI_STYLIZED_FACTS_HORIZON_COMPARISON_VALIDATION_ERROR_ID,
    CliExitCode,
)
from ..documents import _load_json_document
from ..errors import _classify_workflow_command_error
from ..paths import _resolve_path_from_repository_root, resolve_repository_root
from ..rendering import _render_stylized_facts_horizon_comparison_human_output, _write_cli_error, _write_command_output
from ...errors import JaniformError
from ...stylized_facts import run_stylized_fact_horizon_comparison_report_generator


def _run_stylized_facts_horizon_comparison_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
    metric_catalog_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.metric_catalog_json),
    )
    comparison_request_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.comparison_request_json),
    )
    artifact_export_path_argument = cast(Path | None, parsed_arguments.artifact_export_json)
    artifact_export_path = (
        _resolve_path_from_repository_root(repository_root, artifact_export_path_argument)
        if artifact_export_path_argument is not None
        else None
    )

    file_field_path = "metric_catalog_json"
    document_field_path = "metric_catalog_document"
    try:
        metric_catalog_document = _load_json_document(metric_catalog_path, "stylized-fact metric catalog")
        file_field_path = "comparison_request_json"
        document_field_path = "comparison_request_document"
        comparison_request_document = _load_json_document(comparison_request_path, "stylized-fact comparison request")
        artifact_export_document: Optional[dict[str, Any]] = None
        if artifact_export_path is not None:
            file_field_path = "artifact_export_json"
            document_field_path = "artifact_export_document"
            artifact_export_document = _load_json_document(
                artifact_export_path,
                "stylized-fact artifact export config",
            )
        document_field_path = "command_input"
        report_bundle = run_stylized_fact_horizon_comparison_report_generator(
            metric_catalog=metric_catalog_document,
            comparison_request=comparison_request_document,
            execution_mode=cast(str, parsed_arguments.execution_mode),
            artifact_export=artifact_export_document,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_STYLIZED_FACTS_HORIZON_COMPARISON_ENTRYPOINT,
                validation_error_id=CLI_STYLIZED_FACTS_HORIZON_COMPARISON_VALIDATION_ERROR_ID,
                internal_error_id=CLI_STYLIZED_FACTS_HORIZON_COMPARISON_INTERNAL_ERROR_ID,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )
    _write_command_output(
        parsed_arguments,
        json_payload=cast(dict[str, Any], report_bundle),
        human_output_factory=lambda: _render_stylized_facts_horizon_comparison_human_output(
            report_bundle,
            execution_mode=cast(str, parsed_arguments.execution_mode),
        ),
    )
    return int(CliExitCode.SUCCESS)


__all__ = ["_run_stylized_facts_horizon_comparison_command"]
