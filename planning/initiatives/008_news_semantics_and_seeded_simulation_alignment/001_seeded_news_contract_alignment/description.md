## Project 001: Seeded Contract And Consistency Alignment

Align seeded simulation contracts with the model spec and model design document, including remaining non-news consistency gaps found in the implementation audit.

### Context and requirements
- Bring seeded tick simulation news generation in line with the compact model and brainstorming notes.
- Ensure market-state routing for open-window direct jumps and closed-window gaps uses exchange calendar semantics rather than a seasonality proxy when possible.
- Ensure exogenous news mark sampling follows the documented Gamma-Kou coupling structure and matches shared-news mark contracts where feasible.
- Ensure known news, surprise news, and total intensity handling are explicit and testable rather than implicit residual accounting.
- Ensure NIG skew-shift parameter constraints (`delta_beta < 0`) are enforced consistently at model-construction and boundary validation layers, not only calibration stage logic.
- Ensure seeded fine-path event scheduling is compatible with event-time Hawkes semantics expected by the model contract rather than interval-level shortcut behavior.

### Definition of Done
- A single project plan with actionable issue outcomes that resolves the current seeded-vs-shared-news semantic drift and the two remaining seeded consistency gaps.
- New or updated tests for deterministic seeded interval simulation that assert calendar-state routing, mark construction semantics, known/surprise separation behavior, and consistent fine-path scheduling semantics.
- New or updated tests for model parameter validation paths that assert `delta_beta` skew constraint handling is consistent across Rust and Python boundaries.
- Documentation updates or references that close the implementation-to-spec loop for this initiative.

### Agent implementation notes
- Keep existing seeded simulation call sites stable unless explicit refactor is needed to remove incorrect semantics.
- Prefer reusing shared-domain helper functions from `news` and `risk` modules to avoid duplicate logic.
- Keep issue scope focused and do not merge unrelated refactors into this project, but include consistency updates needed for skew constraints and fine-path scheduling semantics.

### Initiative alignment
- This project closes the remaining consistency gaps identified during the implementation audit and supports easier compliance checks against the model contract.
