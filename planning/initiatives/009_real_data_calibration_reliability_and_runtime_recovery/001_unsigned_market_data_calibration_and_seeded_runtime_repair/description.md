## Project 001: Unsigned Market Data Calibration And Seeded Runtime Repair

Repair the end-to-end calibration and simulation workflow for real market data by replacing the current heuristic Hawkes and return-component fitting behavior, removing pathological seeded boundary costs, and automating session handling for common broker-driven datasets.

### Context and requirements
- The implementation must work with unsigned market data. The product requirement is explicit that true signed trade event tapes will not generally be available, and the workflow must therefore estimate directional asymmetry from unsigned tick-price behavior rather than requiring canonical signed trades.
- The current Hawkes stage is not meaningfully calibrated on real data. In `python/janiform/calibration/_market_data_translation.py`, `_build_hawkes_candidate_inputs` produces one heuristic candidate whose parameters are derived directly from observed rate and interarrival summaries. In `crates/core/src/calibration/optimizer_adapter.rs`, the "optimizer" only scores supplied candidates rather than performing a real parameter search. In `crates/core/src/calibration/objective.rs`, the objective is a weighted sum of observed scores, not a mismatch between model-implied and observed statistics.
- The result of that current behavior is visible in the real-data smoke test. Roughly `664,465` observed input ticks over the input window lead to approximately `16.0` million coarse generated events and approximately `16.86` million seeded events for only `96` simulated hours. That event explosion is a primary root cause of both the scale mismatch and the runtime and memory failures.
- The current return-components stage is also weakly identified. Candidate generation on the investigated EUR/USD run produced `intraday_jump_candidate_count = 1`, `material_close_open_gap_count = 0`, and exactly one NIG candidate derived from crude summary moments. The Rust stage then searches only tiny multiplicative families around a few summary statistics. That is not enough evidence to robustly identify news scales, jump scales, gap scales, or NIG return scale on noisy real data.
- The current seeded simulation boundary is too expensive for realistic runs. Rust builds full `fine_path` and `event_path` vectors, PyO3 serializes the entire fine path to one JSON string, Python deserializes the whole payload into object graphs, and Python then reconstructs an `event_tape` from `fine_path`, effectively duplicating the path again.
- Memory is also wasted before seeded simulation starts. The example script keeps all ticks in memory, writes a normalized CSV, and the ingestion path reloads the CSV into tuples of dictionaries and then into normalized record tuples before deduplication and sorting.
- Session handling is currently too manual. The example creates a pragmatic default FX calendar with a `23:59` close and `00:00` reopen on weekdays. That contradicts the intended automation goal and creates admin burden. The system needs session inference or other automatic session materialization from data and workflow context.
- The project must be self-contained and implementation-ready. Engineers should not need access to the original smoke-test conversation to understand what failed and why.

### Definition of Done
- A project plan exists with one issue each for Hawkes calibration from unsigned market data, return-component identification on real data, seeded-simulation runtime and memory repair, and automated session inference.
- Each issue includes the concrete failure mode discovered in the real EUR/USD smoke test, the affected modules, expected implementation direction, acceptance criteria, and suggested test coverage.
- The project explicitly rejects the earlier assumption that full-model calibration can depend on access to true signed trade event tapes.
- The project makes clear that closed-session drift is a secondary issue, not the primary explanation for the observed scale blow-up.
- The resulting plan is aligned to current repository conventions and can be executed without hidden conversation-only context.

### Agent implementation notes
- Pre-v0.1 rules apply. Replace incorrect workflows directly rather than adding compatibility layers around them.
- Keep issue scope focused. The four issues should fit together, but the planner should avoid bundling unrelated refactors into the same implementation task.
- Preserve the distinction between root causes and secondary amplifiers. The main root causes from the investigation are: no real Hawkes fit, weak return-component identification, boundary duplication and serialization, and manual session handling.
- Where possible, identify shared-domain helpers that can be reused instead of creating another heuristic path specific to one example or one data source.

### Initiative alignment
- This project is the first execution vehicle for restoring confidence that Janiform can calibrate and simulate from real unsigned market data without pathological scale, runtime, memory, or session-setup behavior.
