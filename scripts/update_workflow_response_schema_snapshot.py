import io
import json
import sys
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from tempfile import TemporaryDirectory

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from tests.support.workflow_response_contracts import (
    WORKFLOW_RESPONSE_SCHEMA_SNAPSHOT_PATH,
    build_default_calendar_registry,
    build_workflow_response_schema_snapshot,
)


def main() -> int:
    calendar_registry = build_default_calendar_registry()

    with TemporaryDirectory(prefix="janiform-workflow-response-snapshot-") as temporary_directory:
        with redirect_stderr(io.StringIO()), redirect_stdout(io.StringIO()):
            snapshot = build_workflow_response_schema_snapshot(
                calendar_registry,
                Path(temporary_directory),
            )

    snapshot_json = json.dumps(snapshot, indent=2, sort_keys=True) + "\n"

    WORKFLOW_RESPONSE_SCHEMA_SNAPSHOT_PATH.parent.mkdir(parents=True, exist_ok=True)
    WORKFLOW_RESPONSE_SCHEMA_SNAPSHOT_PATH.write_text(snapshot_json, encoding="utf-8")

    print(WORKFLOW_RESPONSE_SCHEMA_SNAPSHOT_PATH)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
