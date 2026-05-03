import json
import re
import shutil
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, Final, Optional

import pandas as pd

from janiform.calibration import JaniformEstimator
from janiform.model import JaniformModel


REPOSITORY_ROOT: Final[Path] = Path(__file__).resolve().parents[1]
DATA_ROOT: Final[Path] = REPOSITORY_ROOT / "data"
TEST_FIXTURE_ROOT: Final[Path] = REPOSITORY_ROOT / "tests" / "fixtures"
EXAMPLE_ARTIFACT_ROOT: Final[Path] = REPOSITORY_ROOT / ".artifacts" / "examples"
EXAMPLE_MODEL_ARTIFACT_ROOT: Final[Path] = EXAMPLE_ARTIFACT_ROOT / "models"
EXAMPLE_OPTION_SURFACE_ARTIFACT_ROOT: Final[Path] = EXAMPLE_ARTIFACT_ROOT / "option_surfaces"
EXAMPLE_MODEL_CACHE_METADATA_SCHEMA_VERSION: Final[str] = (
    "examples.persisted_model_cache_metadata.v1"
)

SYNTHETIC_EURUSD_TICK_DATA_PATH: Final[Path] = (
    DATA_ROOT / "Synthetic_EURUSD_Data_1M_ticks.parquet"
)
CALENDAR_FIXTURE_PATH: Final[Path] = (
    TEST_FIXTURE_ROOT / "calendar" / "calendar_service_regression_matrix.json"
)
HOURLY_FACTOR_FIXTURE_PATH: Final[Path] = (
    TEST_FIXTURE_ROOT / "hourly_factors" / "hourly_factor_regression_matrix.json"
)
STAGED_PREPARED_EXECUTION_PAYLOAD_PATH: Final[Path] = (
    TEST_FIXTURE_ROOT / "calibration" / "staged_prepared_execution_payload.json"
)


@dataclass(frozen=True)
class PersistedExampleModel:
    model: JaniformModel
    model_path: Path
    loaded_from_cache: bool


def load_json_document(path: Path) -> dict[str, Any]:
    raw_document: Any = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw_document, dict):
        raise ValueError(f"expected {path} to contain a JSON object")

    return raw_document


def write_json_document(path: Path, document: dict[str, Any]) -> Path:
    path.write_text(
        json.dumps(document, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    return path


def write_csv_lines(path: Path, rows: Sequence[str]) -> Path:
    path.write_text("\n".join(rows) + "\n", encoding="utf-8")
    return path


def repository_relative_path_string(
    path: Path,
    *,
    repository_root: Path = REPOSITORY_ROOT,
) -> str:
    resolved_path = path.resolve()
    resolved_repository_root = repository_root.resolve()

    try:
        return resolved_path.relative_to(resolved_repository_root).as_posix()
    except ValueError:
        return str(path)


def calendar_fixture_document() -> dict[str, Any]:
    return load_json_document(CALENDAR_FIXTURE_PATH)


def calendar_registry_payload() -> dict[str, Any]:
    raw_payload: Any = calendar_fixture_document()["calendar_registry"]
    if not isinstance(raw_payload, dict):
        raise ValueError("calendar fixture calendar_registry must be a JSON object")

    return raw_payload


def calendar_registry_json() -> str:
    return json.dumps(calendar_registry_payload())


def calendar_regression_cases() -> list[dict[str, Any]]:
    raw_cases: Any = calendar_fixture_document()["cases"]
    if not isinstance(raw_cases, list):
        raise ValueError("calendar fixture cases must be a JSON array")

    return [dict(case) for case in raw_cases]


def hourly_factor_fixture_document() -> dict[str, Any]:
    return load_json_document(HOURLY_FACTOR_FIXTURE_PATH)


def hourly_factor_regression_canonical_cases() -> list[dict[str, Any]]:
    raw_cases: Any = hourly_factor_fixture_document()["canonical_output_cases"]
    if not isinstance(raw_cases, list):
        raise ValueError("hourly-factor canonical_output_cases must be a JSON array")

    return [dict(case) for case in raw_cases]


def load_or_fit_persisted_example_model(
    *,
    tick_data_path: Path,
    market_profile: Optional[str] = None,
    tail_row_count: Optional[int] = None,
    force_refit: bool = False,
    silent: bool = False,
) -> PersistedExampleModel:
    model_path = persisted_example_model_path(
        market_profile=market_profile,
        tick_data_path=tick_data_path,
        tail_row_count=tail_row_count,
    )

    metadata_path = model_path.with_suffix(".metadata.json")

    expected_metadata = _persisted_example_model_cache_metadata(
        market_profile=market_profile,
        tick_data_path=tick_data_path,
        tail_row_count=tail_row_count,
    )

    if not force_refit and model_path.exists() and metadata_path.exists():
        cached_metadata = load_json_document(metadata_path)
        if cached_metadata == expected_metadata:
            return PersistedExampleModel(
                model=JaniformModel.load(model_path),
                model_path=model_path,
                loaded_from_cache=True,
            )

    EXAMPLE_MODEL_ARTIFACT_ROOT.mkdir(parents=True, exist_ok=True)
    estimator = JaniformEstimator(market_profile=market_profile)

    if tail_row_count is None:
        model = estimator.fit(
            tick_data_path=tick_data_path,
            silent=silent,
        )
    else:
        fit_input_path = _materialize_tail_slice(
            tick_data_path=tick_data_path,
            tail_row_count=tail_row_count,
        )
        model = estimator.fit(
            tick_data_path=fit_input_path,
            silent=silent,
        )

    model.save(model_path)
    write_json_document(metadata_path, expected_metadata)

    return PersistedExampleModel(
        model=model,
        model_path=model_path,
        loaded_from_cache=False,
    )


def persisted_example_model_path(
    *,
    tick_data_path: Path,
    market_profile: Optional[str] = None,
    tail_row_count: Optional[int] = None,
) -> Path:
    selection_label = "full" if tail_row_count is None else f"tail_{tail_row_count}"
    market_profile_label = "inferred" if market_profile is None else market_profile
    cache_stem = _cache_token(
        f"{tick_data_path.stem}_{market_profile_label}_{selection_label}"
    )
    return EXAMPLE_MODEL_ARTIFACT_ROOT / f"{cache_stem}.json"


def _cache_token(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", "_", value.lower()).strip("_")


def _materialize_tail_slice(
    *,
    tick_data_path: Path,
    tail_row_count: int,
) -> Path:
    if tick_data_path.suffix.lower() != ".parquet":
        raise ValueError(
            f"persisted example model tail slicing requires a parquet source, received {tick_data_path}"
        )

    with TemporaryDirectory(prefix="janiform-example-fit-") as temporary_directory:
        tail_df: pd.DataFrame = pd.read_parquet(tick_data_path).tail(tail_row_count)
        fit_input_path = Path(temporary_directory) / tick_data_path.name
        tail_df.to_parquet(fit_input_path)

        persisted_fit_input_path = EXAMPLE_MODEL_ARTIFACT_ROOT / (
            f"{_cache_token(f'{tick_data_path.stem}_tail_{tail_row_count}')}.parquet"
        )

        persisted_fit_input_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(fit_input_path), str(persisted_fit_input_path))

        return persisted_fit_input_path


def _persisted_example_model_cache_metadata(
    *,
    tick_data_path: Path,
    market_profile: Optional[str],
    tail_row_count: Optional[int],
) -> dict[str, Any]:
    source_stat = tick_data_path.stat()

    return {
        "schema_version": EXAMPLE_MODEL_CACHE_METADATA_SCHEMA_VERSION,
        "market_profile": market_profile,
        "tick_data_path": repository_relative_path_string(tick_data_path),
        "tick_data_signature": {
            "size_bytes": source_stat.st_size,
            "modified_time_ns": source_stat.st_mtime_ns,
        },
        "tail_row_count": tail_row_count,
    }
