## Python Surface Architecture Reset And Native Data Plane Boundary

### Description
Reset the Python-side architecture so Janiform ships as a professional production library rather than a schema-first wrapper around Python-owned object graphs.

This issue extends the mean-reversion rollout work by addressing the deeper architectural problem that surfaced during JAN-284: the current Python public surface mixes control-plane configuration, bulk empirical data, runtime artifacts, and reports into one tuple-heavy contract style. That style is acceptable for small immutable config objects, but it is not an acceptable canonical boundary for production data-bearing workflows.

The goal of this issue is to make the Python layer explicitly control-plane-first and to move the data plane fully into Rust or into columnar/native-backed boundaries. High-level Python and CLI surfaces must remain explicit and ergonomic, but they must stop owning large evidence payloads as tuples of Python dataclass records.

This is a chore and architecture reset, not a feature. Do not update the initiative or project descriptions as part of this issue.

### Surface classification policy
- `config`
  - Small immutable request, settings, policy, preset, or parameter objects.
  - Python dataclass and enum contracts are acceptable here.
  - Small tuples are acceptable only when they represent short static configuration lists rather than empirical datasets.
- `bulk data`
  - Empirical observations, event tables, calibration evidence, long interval vectors, candidate sets, row-like datasets, or any payload whose natural scale grows with source data size or horizon length.
  - Python tuple-of-record canonical ownership is forbidden for this category.
  - Bulk-data surfaces must be represented by native-owned handles, dataset references, file references, Arrow/NumPy-style columnar buffers, or lazy artifact views.
- `runtime artifact`
  - Fitted models, simulation paths, priced path views, lazy tapes, prepared runtime handles, and reusable workflow outputs.
  - Runtime artifacts may expose lazy accessors and serialization, but should not eagerly materialize bulk data in Python-owned tuples by default.
- `report`
  - Diagnostics, provenance, fit reports, validation summaries, regression reports, and other human or machine-readable summaries.
  - Reports may contain small tuples and nested summaries, but must not duplicate bulk datasets.

### Current Python surface classification

| Surface family | Current public ownership | Classification | Required disposition |
| --- | --- | --- | --- |
| Package root | `janiform` | `config` | Keep minimal. Continue exposing only curated package metadata and no workflow payload ownership. |
| Error taxonomy | `janiform.errors` | `report` | Keep as report and formatting surface only. No workflow data ownership. |
| Calendar queries | `janiform.calendar` | `config`, `runtime artifact` | Keep `CalendarRegistry` and state queries public. Do not let this module become a carrier for tabular session datasets. |
| Shared cross-cutting contracts | `janiform.contracts` | `config` | Keep only small cross-domain contracts here. Do not let this module accumulate bulk evidence payloads. |
| Hourly factor workflow root | `janiform.hourly_factors` | `config`, `runtime artifact`, `bulk data` | Split bulk interval vectors and materialization inputs away from Python-owned tuple contracts. Keep factor contracts and settings public. |
| Seasonality workflow | `janiform.seasonality` | `config`, `report`, `bulk data` | Move event-record and timestamp-series bulk inputs off Python `Sequence[Mapping]` boundaries. Keep estimation settings and reports public. |
| Simulation workflow root | `janiform.simulation` | `config`, `runtime artifact` | Keep as curated convenience surface. Do not expose eager full-path object graphs by default. |
| Coarse simulation | `janiform.simulation.coarse` | `config`, `runtime artifact` | Keep request config public. Ensure interval paths stay lazy or columnar-backed rather than Python tuple ownership. |
| Seeded simulation | `janiform.simulation.seeded` | `config`, `runtime artifact` | Keep request config public. Preserve lazy tape and path access. Do not regress to eager Python record tuples. |
| Shared simulation contracts | `janiform.simulation.shared` | `config` | Keep parameter and error contracts only. |
| Pricing workflow | `janiform.pricing` | `config`, `runtime artifact`, `report`, `bulk data` | Keep payoff and request config public. Move long interval-factor ownership and any dense deviation-state grids away from Python tuple contracts. |
| Risk workflow | `janiform.risk` | `config`, `runtime artifact`, `report`, `bulk data` | Keep specification and summary contracts public. Move long factor vectors and interval evidence off Python-owned tuples. |
| Calibration convenience root | `janiform.calibration` | `config`, `runtime artifact`, `report` | Keep as curated root only. Re-export stable high-level entrypoints, not data-plane evidence models. |
| High-level fit root | `janiform.calibration.fit` | `config`, `runtime artifact`, `report` | Keep `JaniformEstimator`, `JaniformModel`, and `JaniformFitReport` public. They must orchestrate native prep rather than construct Python-owned evidence payloads. |
| High-level estimator | `janiform.calibration.fit.estimator` | `config`, `runtime artifact` | Keep constructor and fit orchestration public. It must become a control-plane adapter only. |
| High-level fitted model | `janiform.calibration.fit.model` | `runtime artifact` | Keep public. Ensure simulation and pricing preparation consume native-backed artifacts and factor handles rather than Python tuple evidence. |
| High-level fit report | `janiform.calibration.fit.report` | `report` | Keep public. Reports must summarize preparation and stage execution without embedding bulk evidence. |
| Market-data workflow root | `janiform.calibration.market_data` | `config`, `runtime artifact`, `report` | Keep request entrypoint public. The prepared result must stop exposing Python-owned staged evidence payloads as the normal path. |
| Market-data request contracts | `janiform.calibration.market_data.contracts` | `config` | Keep public. Restrict this module to small request and policy contracts only. |
| Market-data request serialization | `janiform.calibration.market_data.request` | `config` | Keep public. Requests should remain file- and policy-oriented rather than embedding empirical evidence arrays. |
| Market-data ingestion helpers | `janiform.calibration.market_data.ingestion` | `report` | Keep schema-inspection and source-summary contracts public. Do not expand this into a Python data loading surface. |
| Market-data translation | `janiform.calibration.market_data.translation` | `runtime artifact`, `report`, `bulk data` | Reset. Translation must not expose Python-owned `StagedCalibrationExecutionInput` objects containing large evidence tuples on the default path. |
| Market-data prepared workflow | `janiform.calibration.market_data.workflow` | `runtime artifact`, `report` | Keep public, but change the prepared object to carry native-backed preparation handles and summaries rather than staged evidence payloads. |
| Staged calibration convenience root | `janiform.calibration.staged` | `config`, `report`, `runtime artifact` | Keep expert-only. Clarify that this is the advanced orchestration boundary, not the recommended production workflow boundary. |
| Staged shared contracts | `janiform.calibration.staged.contracts` | `config` | Keep stage-selection and execution-mode contracts public. |
| Staged execution input payloads | `janiform.calibration.staged.inputs` | `config`, `bulk data` | Major reset. Split config from evidence. Python dataclass tuples of observations, candidates, or lower-frequency grids must be replaced. |
| Staged request | `janiform.calibration.staged.request` | `config` | Keep public. Request stays small and declarative. |
| Staged results | `janiform.calibration.staged.results` | `report`, `runtime artifact` | Keep public. Results may reference artifacts and summaries but must not duplicate evidence payloads. |
| Staged runtime bridge | `janiform.calibration.staged.runtime` | `runtime artifact` | Keep public runner entrypoint. It must accept native-backed or file/handle-based evidence ownership rather than Python tuple payloads. |
| Preset workflow root | `janiform.calibration.presets` | `config` | Keep public. No bulk data. |
| Preset references | `janiform.calibration.presets.references` | `config` | Keep public. |
| Preset registry | `janiform.calibration.presets.registry` | `config`, `runtime artifact` | Keep public as registry/runtime helper. No bulk data. |
| Preset resolution | `janiform.calibration.presets.resolution` | `config`, `report` | Keep public. |
| Session workflow root | `janiform.calibration.sessions` | `config`, `report` | Keep public. |
| Session contracts | `janiform.calibration.sessions.contracts` | `config`, `report` | Keep public. |
| Session resolution helpers | `janiform.calibration.sessions.resolution` | `runtime artifact` | Keep helper-only. No tabular ownership. |
| Validation workflow | `janiform.validation` | `report`, `runtime artifact` | Keep public. Validation harnesses should load artifacts and references, not Python-owned bulk payloads. |
| Stylized-facts workflow | `janiform.stylized_facts` | `report`, `runtime artifact` | Keep public. Input contracts may stay small, but long comparison datasets should be artifact-backed. |
| CLI root | `janiform.cli` | `config`, `report` | Keep public as command application surface only. |
| Calibration CLI commands | `janiform.cli.commands.calibration` | `config`, `report` | Keep as adapters. No bulk data ownership. |
| Development CLI commands | `janiform.cli.commands.development` | `config`, `report` | Keep as adapters. No bulk data ownership. |
| Model workflow CLI commands | `janiform.cli.commands.model_workflows` | `config`, `report` | Keep as adapters. They must express high-level config, not stage evidence payloads. |
| Simulation CLI commands | `janiform.cli.commands.simulation` | `config`, `report` | Keep as adapters. |

### Required architecture reset

### 1. Define one explicit Python boundary policy and enforce it
- Update the Python public-surface guide so every public module is documented as one or more of `config`, `bulk data`, `runtime artifact`, or `report`.
- Add an implementation-facing guide that states:
  - Python is the control plane.
  - Rust is the data plane.
  - Bulk empirical datasets must remain native-owned or columnar-backed.
  - Public Python dataclass contracts are for small config and summary objects only.
- Add a review checklist that explicitly rejects new Python tuple-of-record bulk evidence surfaces.

### 2. Remove Python-owned bulk evidence from the default calibration path
- Change the default high-level path rooted at `JaniformEstimator.fit(...)` so Python never materializes the canonical calibration evidence model as tuples of observation records.
- Replace the current flow:
  - market-data request
  - Python translation result with `StagedCalibrationExecutionInput`
  - staged execution input with Python-owned stage evidence tuples
- With a new flow:
  - market-data request
  - native preparation and translation
  - prepared calibration handle or artifact reference
  - staged execution invoked against that handle or artifact
- `PreparedMarketDataCalibration` must become a runtime artifact and report object, not a Python-owned evidence container.

### 3. Split staged calibration config from staged calibration evidence
- Keep `janiform.calibration.staged.request` and `janiform.calibration.staged.contracts` as the config boundary.
- Replace the evidence-bearing portions of `janiform.calibration.staged.inputs` with one of the following canonical shapes:
  - native-backed preparation handles
  - dataset references
  - file references to typed artifact payloads
  - Arrow or NumPy-backed columnar inputs where a Python-side direct boundary is still intentionally supported
- Remove Python dataclass tuple ownership for:
  - seasonality event records and dense timestamp inputs
  - Hawkes candidate evidence series
  - return-components observed open-session returns, jump candidates, boundary observations, and similar empirical evidence
  - mean-reversion lower-frequency observation grids
  - any future staged evidence payload that scales with dataset size or horizon length
- Keep schema identifiers at `v1` and replace old payloads directly. Do not add compatibility shims.

### 4. Reset market-data translation to return prepared artifacts rather than staged payload graphs
- `translate_market_data_request_to_staged_execution_input(...)` should be superseded by a surface that returns:
  - a prepared native-backed calibration plan
  - preparation diagnostics
  - provenance
  - session resolution
  - optional references to persisted prepared artifacts
- The public default path must stop returning a full `StagedCalibrationExecutionInput` object for normal use.
- If an expert low-level translation-to-staged-input entrypoint remains public, it must be clearly repositioned as advanced-only and must consume artifact references or native handles for bulk evidence rather than Python tuples.
- The mean-reversion observation derivation added during JAN-284 follow-up must move fully into Rust. Python should only request the mean-reversion policy and receive diagnostics and fitted results back.

### 5. Audit every existing bulk-data-shaped Python surface and redesign it explicitly
- `janiform.seasonality`
  - Move `Sequence[Mapping[str, Any]]` style event-record ingestion off the primary public path.
  - Introduce file-backed, artifact-backed, or columnar-backed ingestion boundaries.
- `janiform.hourly_factors`
  - Keep settings, provider descriptors, and factor contracts public.
  - Replace long timestamp and factor sequences as the canonical ownership shape with columnar or artifact-backed inputs when they are data-bearing rather than tiny examples.
- `janiform.pricing`
  - Keep payoff config, request config, and run results public.
  - Audit interval-factor tuples and anchored-state auxiliary grids.
  - Long horizon factor ownership must use handles, factor contracts, or columnar inputs rather than Python record tuples.
- `janiform.risk`
  - Apply the same rule as pricing for long factor vectors and interval evidence.
- `janiform.simulation`
  - Preserve lazy artifact views.
  - Ensure no public response surface regresses into eager Python tuple ownership for large path or tape data.

### 6. Keep high-level Python and CLI surfaces explicit without exposing low-level bulk evidence
- `JaniformEstimator`
  - Must accept only config-like inputs plus file paths, preset references, and workflow policies.
  - Must not build or expose canonical bulk evidence payloads.
- `JaniformModel`
  - Must prepare pricing, risk, and simulation requests using native-backed runtime artifacts or factor handles.
  - High-level methods must remain explicit about required observed-state boundary inputs and factor policies.
- CLI surfaces
  - Must accept file paths, policy flags, and explicit config only.
  - Default commands must not expose or require direct Python-staged evidence payload assembly.
  - Advanced expert commands may still expose raw staged orchestration, but bulk evidence must enter through artifact references or typed dataset files rather than giant JSON tuple payloads.

### 7. Reclassify reports and runtime artifacts so they stop duplicating data-plane payloads
- `JaniformFitReport`, `StagedCalibrationResponse`, validation harness reports, pricing outputs, and stylized-fact reports must contain:
  - summaries
  - diagnostics
  - provenance
  - artifact references
  - native-backed handle metadata when applicable
- They must not embed bulk empirical inputs or large path datasets except where a bounded deterministic fixture is intentionally part of a test-only artifact.

### 8. Update documentation, examples, and guardrails to reflect the new architecture
- Rewrite the Python public-surface map so it describes the control-plane/data-plane split explicitly.
- Add or update a design guide for the Python boundary reset.
- Update README and examples so the recommended workflow no longer implies that Python contract models own large empirical payloads.
- Update the public API snapshot and any workflow snapshots so the supported surface reflects the new architecture.
- Add explicit contributor guidance forbidding new bulk-data Python tuple contracts.

### Exact surfaces that should be updated and how

### High-level workflow updates
- Update [`python/janiform/calibration/fit/estimator.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/fit/estimator.py)
  - keep constructor and `fit(...)` config-oriented
  - remove any dependency on Python-owned staged evidence objects in the default path
  - consume a prepared calibration handle or artifact reference returned by native preparation
- Update [`python/janiform/calibration/fit/model.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/fit/model.py)
  - ensure runtime preparation reads native-backed factor artifacts or handles
  - forbid reconstruction of large factor or evidence datasets as Python tuples during normal pricing or simulation preparation
- Update [`python/janiform/calibration/fit/report.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/fit/report.py)
  - keep summary-only report semantics
  - replace any embedded evidence duplication with artifact or handle references

### Market-data preparation updates
- Update [`python/janiform/calibration/market_data/translation.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/market_data/translation.py)
  - replace the current staged-input-returning default path with a prepared-artifact or handle-returning path
  - move mean-reversion observation-grid derivation fully into native code
  - preserve diagnostics and provenance, but stop returning Python-owned evidence tuples
- Update [`python/janiform/calibration/market_data/workflow.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/market_data/workflow.py)
  - redefine `PreparedMarketDataCalibration` as a runtime artifact plus report surface
  - expose handle metadata and artifact references rather than raw evidence payloads
- Update [`python/janiform/calibration/market_data/contracts.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/market_data/contracts.py)
  - keep only config-like request and policy contracts public
  - move any accidental data-bearing ownership out of this module

### Staged calibration boundary updates
- Update [`python/janiform/calibration/staged/inputs.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/staged/inputs.py)
  - split config from evidence explicitly
  - remove tuple-of-record ownership for bulk evidence models
  - replace evidence-bearing payloads with handle, artifact-reference, file-reference, or columnar-backed forms
- Update [`python/janiform/calibration/staged/request.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/staged/request.py)
  - keep only declarative execution selection and policy controls
- Update [`python/janiform/calibration/staged/results.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/staged/results.py)
  - keep report and artifact-reference semantics only
- Update [`python/janiform/calibration/staged/runtime.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/staged/runtime.py)
  - accept the new prepared data-plane boundary and remove dependence on Python-owned bulk evidence payload graphs

### Pricing, risk, seasonality, and hourly-factor updates
- Update [`python/janiform/pricing.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/pricing.py)
  - audit interval-factor and auxiliary recursion input ownership
  - keep config and run result contracts public
  - move any long-horizon bulk data ownership behind factor handles or columnar inputs
- Update [`python/janiform/risk.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/risk.py)
  - apply the same separation as pricing
- Update [`python/janiform/seasonality.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/seasonality.py)
  - replace direct Python row-sequence ownership on the recommended path
  - keep settings, summaries, and deterministic results public
- Update [`python/janiform/hourly_factors.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/hourly_factors.py)
  - keep materialization settings and factor contracts public
  - move data-bearing sequence ownership behind file, handle, or columnar boundaries

### CLI updates
- Update [`python/janiform/cli/commands/model_workflows.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/cli/commands/model_workflows.py)
  - keep high-level fit and simulate commands config-only
  - ensure advanced staged workflows use artifact-backed expert boundaries
- Update [`python/janiform/cli/commands/calibration.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/cli/commands/calibration.py)
  - remove any assumption that staged evidence is naturally expressed as Python tuple contracts
- Update [`python/janiform/cli/commands/simulation.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/cli/commands/simulation.py)
  - preserve artifact-backed and lazy output semantics
- Update CLI docs so expert-only raw orchestration is clearly separated from the recommended production path.

### Documentation and governance updates
- Update README and the following guides:
  - `docs/development/guides/python_public_api_surface_map_and_namespace_policy.md`
  - `docs/development/guides/python_api_contract_compatibility_and_version_guardrails.md`
  - `docs/development/guides/cli_command_reference_and_examples.md`
  - any pricing, risk, simulation, or calibration guides that currently normalize Python-owned tuple evidence as the standard workflow shape
- Add a new implementation-facing guide for the Python control-plane and Rust data-plane boundary if one does not already exist.

### Acceptance Criteria
- Every public Python surface family is explicitly classified as `config`, `bulk data`, `runtime artifact`, or `report` in the updated docs.
- The recommended high-level calibration path no longer returns or depends on Python-owned staged evidence tuples.
- `janiform.calibration.staged.inputs` no longer defines tuple-of-record bulk evidence as the canonical public ownership model.
- Mean-reversion lower-frequency observation materialization is native-owned on the production path.
- Market-data translation and preparation expose runtime handles or artifact references plus diagnostics and provenance, not Python-owned evidence graphs.
- Pricing, risk, seasonality, and hourly-factor surfaces have an explicit rule for bulk-data ownership and no new tuple-heavy regressions.
- CLI defaults remain ergonomic and explicit while avoiding any need for users to construct giant JSON evidence payloads.
- Public API snapshots, examples, and docs all describe the same control-plane/data-plane split.
- The full Rust test suite takes no longer than 60 seconds to run.
- The full Python test suite takes no longer than 60 seconds to run.

### Unit tests
- Public contract tests that enforce the new classification-driven boundaries for calibration, pricing, risk, seasonality, and hourly-factor surfaces.
- High-level fit tests proving the default path stays config-only on the Python side.
- Translation and preparation tests proving prepared artifacts expose diagnostics and references without embedding evidence payloads.
- Snapshot and guardrail tests proving that tuple-backed bulk evidence models are removed or demoted from the supported public surface.
- CLI contract tests proving high-level commands accept config-oriented arguments and expert commands consume artifact-backed staged inputs when needed.

### Integration tests
- End-to-end high-level calibration workflows that exercise native preparation plus high-level fit without exposing Python-owned staged evidence graphs.
- End-to-end pricing, risk, and simulation workflows that consume runtime artifacts and factor handles rather than tuple-heavy Python evidence payloads.
- Documentation regression tests for README and command-reference examples after the public workflow changes.
- Validation harness tests that load artifacts and summaries without re-materializing bulk empirical evidence in Python.

### Notes/Constraints
- This is a breaking pre-v0.1 cleanup. Do not add compatibility shims, legacy aliases, or dual old/new payload support.
- Keep schema identifiers at `v1`.
- Prefer direct replacement of weak public boundaries over layered wrappers.
- The default high-level path must be production-first. Expert staged surfaces may remain public, but they must no longer normalize Python-owned tuple evidence as the canonical shape.
- Do not treat this as a documentation cleanup. The code, tests, docs, examples, and guardrails must move together in the same architecture-reset sequence.
