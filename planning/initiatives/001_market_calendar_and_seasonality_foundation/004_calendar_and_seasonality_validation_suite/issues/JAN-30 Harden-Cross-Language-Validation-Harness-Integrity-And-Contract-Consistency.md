**Summary**

We now have a shared deterministic regression harness for calendar state and hourly factor validation, but there are four structural liabilities that increase long-term production risk. This issue consolidates them into one reliability hardening pass so Rust and Python validation paths stay deterministic, auditable, and maintainable.

**Context**

Janiform is being positioned as a production-ready quantitative library for external finance users. Validation and fixture handling are part of the trust model. Current implementation has gaps in fixture integrity coverage, duplicated regression metadata, inconsistent field naming in seasonality source contracts, and weak schema guarantees in the Python wrapper boundary.

**Observed liabilities (current state)**

1. Hourly regression fixtures are outside shared manifest/hash validation. `tests/fixtures/validation/validation_fixture_manifest.json` only tracks calendar fixtures. Hourly fixture files are loaded directly in `tests/conftest.py` and `crates/core/tests/hourly_factor_regression_fixture_matrix.rs`.
2. Regression metadata is duplicated across Rust and Python tests. Required tags and expected fixture ID sets are repeated in:
   - `tests/test_calendar.py`
   - `crates/core/tests/calendar_regression_fixture_matrix.rs`
   - `tests/test_hourly_factor_regression_matrix.py`
   - `crates/core/tests/hourly_factor_regression_fixture_matrix.rs`
3. Seasonality source timestamp field naming is inconsistent. Python serialization emits `source_timestamp_utc` in `python/janiform/hourly_factors.py`, while fixture/test paths use `source_timestamps_utc`. PyO3 currently accepts both in `crates/pybind/lib.r`, which hides schema drift.
4. Python harness wrapper is weakly typed at runtime. `python/janiform/validation.py` passes raw mappings and casts parsed JSON to `TypedDict` without structural validation.

**Scope**

1. Extend the shared validation manifest to include hourly factor regression fixture(s), including content hash verification in both Rust and Python loader paths.
2. Remove duplicated required tag / expected fixture ID constants from language-specific tests by using a single canonical metadata source from fixtures.
3. Normalize seasonality timestamp field naming to one canonical key across fixture docs, Python serialization, PyO3 parsing, and tests.
4. Add strict input/output schema validation for `run_deterministic_regression_harness` Python wrapper (fail fast with explicit errors on malformed shape/types).

**Non-goals**

1. Changing model math or calibration logic.
2. Adding new fixture scenarios beyond what is needed to enforce the new structure.

**Acceptance criteria**

1. Hourly fixtures are listed in `tests/fixtures/validation/validation_fixture_manifest.json` with hash checks enforced by both Rust and Python fixture loaders.
2. Rust and Python regression tests derive required-tag and stable-ID expectations from one canonical fixture metadata source instead of duplicated constants.
3. Only one seasonality timestamp field name is accepted in harness/provider contracts, with all callers updated.
4. `python/janiform/validation.py` validates request/response schema structure explicitly before use.
5. Tests include failure-path coverage for malformed manifest entry, malformed canonical metadata, invalid seasonality timestamp key, and malformed harness report payload.
6. Determinism tests still pass in both Rust and Python.

**Suggested files to touch**

- `tests/fixtures/validation/validation_fixture_manifest.json`
- `tests/conftest.py`
- `tests/fixture_validation.py`
- `crates/core/tests/support/validation_fixture.rs`
- `crates/core/tests/hourly_factor_regression_fixture_matrix.rs`
- `tests/test_hourly_factor_regression_matrix.py`
- `python/janiform/hourly_factors.py`
- `crates/pybind/lib.r`
- `python/janiform/validation.py`

**Verification commands**

```
cargo test --workspace
conftest.py
pytest
```
