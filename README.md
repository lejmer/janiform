# Janiform

Janiform is a compact high-frequency asset return model that combines seasonal asymmetric Hawkes trade activity with NIG business-time shocks and marked news jumps (including closed-market gap effects). The Rust core provides the computation engine. The Python package exposes an importable API and CLI via PyO3 bindings.

The name *Janiform* comes from the Roman god *Janus* who is a god with two faces (one looking forward, one backward). Janus is also the god of transitions and duality. Asymmetry is built into the model:
- Two Lévy clocks
- Two intensities
- Two excitation channels

It's Jani*form* because it's about form: a system whose geometry is essentially bifacial.

## Baseline Support

- Python: `>=3.12`
- Rust: `>=1.89`

## Tooling

- Build backend: `maturin`
- Python-Rust bindings: `PyO3`

## Project Layout

- `crates/core/`: Rust core library
- `crates/pybind/`: PyO3 extension module (`janiform._native`)
- `python/janiform/`: Python package and CLI entrypoints
- `tests/`: Python tests and shared fixtures
- `docs/`: Design and development documentation

## Quickstart

```bash
maturin develop
python3 -m janiform --help
janiform --help
```

## Documentation

- Design model and rationale: `docs/design/janiform_design_brainstorm_notes.md`
- Calendar and seasonality API contracts: `docs/development/guides/calendar_and_seasonality_api_contracts.md`
- Hawkes parameter validation report: `docs/development/guides/hawkes_parameter_validation_report.md`
- Hawkes failure diagnostics report: `docs/development/guides/hawkes_failure_diagnostics_report.md`
- NIG characteristic exponent guardrails: `docs/development/guides/nig_characteristic_exponent_guardrails.md`
- NIG cross-engine interface and reference fixtures: `docs/development/guides/nig_cross_engine_interface_and_reference_fixtures.md`
- Return decomposition API contract: `docs/development/guides/return_decomposition_api_contract.md`
- Vanilla option pricing contract: `docs/development/guides/vanilla_option_pricing_contract.md`
  - Includes `interval_delta` (`Delta`) semantics with hourly (`1.0`) intraday default and configurable non-hourly support when compatible factors are supplied
- Sufficient-statistics risk sampler contract: `docs/development/guides/sufficient_statistics_risk_sampler_contract.md`
- Pricing diagnostics and calibration hooks schema: `docs/development/guides/pricing_diagnostics_and_calibration_hooks_schema.md`
- Calibration optimizer adapter and objective composition schema: `docs/development/guides/calibration_optimizer_adapter_and_objective_composition_schema.md`
- Calibration artifact manifest schema and versioning policy: `docs/development/guides/calibration_artifact_manifest_schema_and_versioning_policy.md`
- Calibration dataset snapshot and input provenance schema: `docs/development/guides/calibration_dataset_snapshot_and_input_provenance_schema.md`
- Staged calibration failure playbook: `docs/development/guides/staged_calibration_failure_playbook.md`
- Open closed gap summary and diff report contract: `docs/development/guides/open_closed_gap_summary_and_diff_report_contract.md`
- Scale-consistency threshold policy and decision contract: `docs/development/guides/scale_consistency_threshold_policy_and_decision_contract.md`
- Stylized fact metric catalog schema: `docs/development/guides/stylized_fact_metric_catalog_schema.md`
- Stylized fact horizon comparison report schema: `docs/development/guides/stylized_fact_horizon_comparison_report_schema.md`
- Affine oracle solver diagnostics and telemetry schema: `docs/development/guides/affine_oracle_solver_diagnostics_schema.md`
- Seeded tick event tape schema: `docs/development/guides/seeded_tick_event_tape_schema.md`
- Scale consistency report and drift diagnostics schema: `docs/development/guides/scale_consistency_report_and_drift_diagnostics_schema.md`
- Validation fallback policy: `docs/development/guides/validation_fallback_policy.md`
- CI tiered execution strategy: `docs/development/guides/ci_tiered_execution_strategy.md`
- Python coding conventions: `docs/development/guides/python_style_guide.md`
- Planning artifacts: `docs/planning/`

The detailed calendar and seasonality sections were moved out of this README into the development guide above to keep the repo root documentation concise.

## Developer Commands

```bash
python3 -m py_compile python/janiform/*.py tests/conftest.py
cargo check --workspace
python3 -m janiform regression open-closed-gap-leakage
```
