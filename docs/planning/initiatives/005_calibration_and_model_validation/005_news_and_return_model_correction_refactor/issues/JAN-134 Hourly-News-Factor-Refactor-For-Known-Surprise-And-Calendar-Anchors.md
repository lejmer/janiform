## Hourly News Factor Refactor For Known Surprise And Calendar Anchors

### Description
Refactor hourly news-factor materialization and provider contracts so known-news calendar anchors and surprise-news background intensity are represented explicitly.

### Implementation details
- Refactor `crates/core/src/hourly_factors.rs` news materialization contracts to carry component-level intensity controls for known and surprise news.
- Update `HourlyNewsIntensityMaterializationOutput` and metadata so component contributions are observable and auditable.
- Refactor `crates/core/src/hourly_factor_provider.rs` source enums so materialized news inputs represent known-news calendar anchors and surprise-news controls directly.
- Remove or replace assumptions that deterministic multipliers and closed-window shares define the core news process.
- Preserve canonical interval indexing and calendar-boundary semantics while changing only news-intensity composition behavior.
- Update deterministic regression fixtures and expected-output contracts that currently assume closed-window baseline plus open override semantics.

### Acceptance Criteria
- Hourly-factor contracts support known-news and surprise-news intensity inputs with deterministic aggregation policy.
- Materialization semantics do not couple trade-seasonality factors to news-intensity generation.
- Calendar-anchored known-news inputs and surprise-intensity controls are represented explicitly in provider APIs.
- Output diagnostics include component-level metadata for known and surprise contributions.

### Unit tests
- Materialization correctness for known-only, surprise-only, and combined scenarios.
- Validation failures for malformed calendar-anchor inputs and invalid surprise-intensity parameters.
- Deterministic output and metadata ordering on fixed fixtures.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Keep provider interfaces aligned with shared hourly-factor contracts used by simulation, pricing, and calibration.
