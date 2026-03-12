## Project 008: High-Level User API And Preset System

Deliver the default Janiform user surface around high-level fit and simulate workflows, first-class preset registries, and fitted-model objects, so most users start from market data and named market defaults instead of staged execution contracts and manual runtime wiring.

### Context and requirements
- Establish a deliberate two-layer product surface: a high-level default API for most users and a low-level advanced API for power users, debugging, and internal orchestration.
- Introduce first-class preset categories for session presets, known-news presets, and market profile presets, with inspectable provenance and deterministic composition rules.
- Make automatic session inference the default session-resolution path when workflow context provides enough evidence, while preserving explicit session presets and explicit session JSON as override paths.
- Keep known-news defaults opt-in. Market profiles may recommend known-news presets, but they must not silently enable materially behavior-changing news inputs unless the selected profile contract says so explicitly.
- Define the high-level Python object model around `JaniformEstimator`, `JaniformModel`, and `JaniformFitReport`, and align the CLI to the same mental model of choose presets, fit, inspect, and simulate.
- Ensure preset resolution, session inference, and user overrides feed one canonical resolved-configuration model with explicit provenance instead of parallel hidden stacks.
- Preserve staged calibration contracts and manual runtime contracts as advanced surfaces, not the default onboarding path.

### Definition of Done
- Planning defines a coherent default Python and CLI surface for the high-level workflow described in `docs/design/high_level_user_api_and_preset_strategy_plan.md`.
- Issue planning covers preset reference contracts, built-in and user registry behavior, resolved-configuration and provenance rules, the high-level estimator and fitted-model API, model-side simulation and pricing methods, CLI preset discovery and high-level workflow commands, and docs migration.
- The project defines how bundled presets are consumed as maintained runtime artifacts rather than informal examples, and it stays aligned with deterministic preset generation planning.
- Session inference, named presets, and explicit user overrides are integrated into one canonical resolution story with clear precedence and diagnostics.
- The resulting plan makes the advanced low-level surfaces intentionally available without leaving them as the primary public UX for v0.1.

### Agent implementation notes
- Keep preset resolution deterministic and inspectable. If Janiform inferred, merged, or replaced a preset-derived component, the fitted model and fit report must say so.
- Do not create a second calibration or simulation engine for the high-level UX. The new default surface should translate into the canonical staged calibration and runtime execution paths.
- Prefer a small, curated bundled preset set for v1 over broad but weak coverage.
- Coordinate runtime preset packaging and provenance with `docs/design/programmatic_preset_generation_plan.md` rather than inventing a separate bundled-default maintenance path.
- Reuse the existing calendar infrastructure and the session-inference planning under Initiative 009 instead of introducing another session-resolution stack.

### Initiative alignment
- Extends Initiative 006 from explicit low-level contracts and market-data ingestion into the intended default Janiform experience for normal users.
- Reduces the risk that v0.1 ships technically correct internals while still feeling staged-contract-first and operationally unnatural for users who just want to fit and simulate with sensible defaults.
