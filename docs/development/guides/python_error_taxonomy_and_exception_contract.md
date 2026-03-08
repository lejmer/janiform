# Python Error Taxonomy And Exception Contract

This guide defines the shared Python-facing exception contract for Janiform workflow APIs and
CLI commands.

## Scope

The contract covers:

- typed validation failures for public request and response contracts
- runtime execution failures at workflow boundaries
- integration failures when Python cannot safely dispatch to or decode from a dependency
- deterministic error identifiers and message formatting shared by importable APIs and CLI output

The goal is to keep failures actionable for callers without leaking raw internal exceptions or
Rust implementation details.

## Shared taxonomy

Shared error classes live in `janiform.errors`:

- `JaniformError`
- `JaniformValidationError`
- `JaniformExecutionError`
- `JaniformIntegrationError`
- `JaniformErrorCategory`
- `JaniformErrorDetails`
- `ContractValidationFailure`

Validation errors for workflow modules subclass `JaniformValidationError` and expose
`failure`, which is a `ContractValidationFailure`.

## Required metadata

Every public Janiform error includes deterministic metadata:

- `error_id`
- `workflow`
- `entrypoint`
- `message`

Validation failures also include:

- `field_path`

Errors may also include:

- `context`

`context` is a deterministic mapping of additional safe metadata such as `exchange_code`,
`module_name`, or `source_category`.

## Message format

`str(error)` is deterministic and uses one shared format:

```text
[error_id] category="..." workflow="..." entrypoint="..." field_path="..." context={...} message="..."
```

`field_path` is omitted when not applicable.
`context` is omitted when empty.

The CLI prints this same formatted error string to stderr so scripts and importable callers see
the same identifiers and classification.

## Workflow contract

### Simulation

Public module: `janiform.simulation`

Validation failures:

- class: `SeededTickSimulationValidationError`
- identifier: `simulation.validation.contract`
- failure metadata:
  - `workflow="simulation"`
  - `entrypoint="contract"` for request and response model validation
  - `entrypoint="load_hourly_factor_contract_for_simulation"` for shared hourly-factor loading

Execution failures:

- class: `JaniformExecutionError`
- identifier: `simulation.execution.run_failed`
- entrypoint: `run_seeded_tick_event_simulation`

Integration failures:

- class: `JaniformIntegrationError`
- identifiers:
  - `simulation.integration.native_module_unavailable`
  - `simulation.integration.invalid_native_response`
  - `simulation.integration.hourly_factor_contract_unavailable`

Expected failure modes:

- invalid request or response payload fields
- invalid calendar, seasonality, or hourly-factor inputs provided to the loader helper
- native simulation dispatch failures
- malformed native response payloads

### Pricing

Public module: `janiform.pricing`

Validation failures:

- class: `VanillaOptionPricingValidationError`
- identifier: `pricing.validation.contract`
- entrypoints:
  - `contract`
  - `load_hourly_factor_contract_for_pricing`

Integration failures:

- class: `JaniformIntegrationError`
- identifier: `pricing.integration.hourly_factor_contract_unavailable`

Expected failure modes:

- invalid pricing request or response payloads
- invalid hourly-factor loader inputs
- unavailable native hourly-factor integration

Current note:

The public pricing module currently exposes contract validation and hourly-factor loading helpers.
It does not yet expose a Python-side execution entrypoint for pricing runs, so no pricing-specific
execution error identifier is currently emitted.

### Risk

Public module: `janiform.risk`

Validation failures:

- class: `SufficientStatisticsRiskValidationError`
- identifier: `risk.validation.contract`
- entrypoint: `contract`

Expected failure modes:

- invalid run request fields
- invalid path payload fields
- mismatched canonical metadata or diagnostics fields

Current note:

The public risk module currently exposes contract validation and serialization helpers only.

### Calibration

Public module: `janiform.calibration`

Validation failures:

- class: `CalibrationValidationError`
- identifier: `calibration.validation.contract`
- entrypoint: `contract`

Expected failure modes:

- invalid staged calibration request fields
- invalid staged calibration response fields
- invalid execution-mode, handoff, or resume combinations

Current note:

The public calibration module currently exposes typed contracts and serialization helpers only.

### CLI

Public module: `janiform.cli`

Current command coverage:

- `janiform dev regression open-closed-gap-leakage`

Validation failures:

- class: `JaniformValidationError`
- identifier: `cli.validation.open_closed_gap_leakage.invalid_input`
- entrypoint: `dev.regression.open_closed_gap_leakage`

Execution failures:

- class: `JaniformExecutionError`
- identifier: `cli.execution.open_closed_gap_leakage.failed`
- entrypoint: `dev.regression.open_closed_gap_leakage`

Expected failure modes:

- missing or malformed fixture documents
- invalid fixture payload shape
- unexpected command execution failures

## Caller guidance

- Catch workflow-specific validation exceptions when you need field-level request or response
  feedback.
- Catch `JaniformExecutionError` when the workflow started but could not complete.
- Catch `JaniformIntegrationError` when Python could not safely call into or decode from a
  dependency boundary.
- Use `error_id` as the stable programmatic discriminator. Do not parse free-form text from
  `message`.
