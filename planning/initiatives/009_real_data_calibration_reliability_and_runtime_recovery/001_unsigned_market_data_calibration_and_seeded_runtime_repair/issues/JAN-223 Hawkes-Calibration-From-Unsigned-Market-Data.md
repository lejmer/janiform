## Hawkes Calibration From Unsigned Market Data

### Description
Replace the current heuristic Hawkes candidate seeding path with a real calibration workflow that can fit event-rate and asymmetry behavior from unsigned market data. The target is not to recover a true signed trade tape. The target is to estimate a bivariate excitation model that reproduces observed activity clustering and directional asymmetry well enough to control return skew and event intensity realistically on real broker tick data.

The current implementation is not performing a meaningful Hawkes calibration:
- In `python/janiform/calibration/_market_data_translation.py`, `_build_hawkes_candidate_inputs` constructs exactly one candidate from observed data summaries such as open tick count, interarrival coefficient of variation, and known-news ratio.
- In `crates/janiform/src/calibration/optimizer_adapter.rs`, `run_constrained_optimizer_adapter` only evaluates supplied candidates. It does not search a continuous parameter space or run a model-implied fit loop.
- In `crates/janiform/src/calibration/objective.rs`, `compose_calibration_weighted_objective` sums `weight * observed_score`. For the Hawkes stage, those observed scores are the raw observed moments themselves, not errors between model-implied moments and observed moments.
- In the investigated EUR/USD run, this led to a model that generated approximately `7.43` million buy events and approximately `8.60` million sell events in the coarse path over `96` simulated hours, despite only `664,465` observed input ticks over a much longer raw input window.

That event-count blow-up is the strongest root cause behind the unrealistic seeded event volume, the slow simulation runtime, and much of the return-scale explosion. A real-data calibration workflow cannot continue to treat one heuristic seed as a fitted Hawkes model.

### Implementation details
- Redesign the Hawkes market-data calibration path so it fits parameters against model-implied diagnostics rather than scoring raw observed summaries directly.
- Support unsigned data as the primary input contract for this workflow. Do not require access to true signed event tapes. Instead, define an explicit directional-label estimation policy from price evolution or other available unsigned inputs, and make that policy part of the calibration contract and diagnostics.
- Replace `_build_hawkes_candidate_inputs` with a calibration preparation step that can generate either:
  - a parameter search space plus fitting targets, or
  - a deterministic family of candidate parameterizations that are scored against simulated or analytically implied moments rather than raw observations.
- Ensure the objective compares model outputs to observed targets. At minimum, include open-session event rate, buy/sell imbalance proxy, interarrival dispersion, clustering signature, and session-state behavior. The exact target set can evolve, but the objective must depend on the candidate parameters through model implications.
- Review the meaning of `lambda_bar` under the two-sided Hawkes model. The current seed effectively uses half the observed open-session rate as the per-side baseline and then layers self-excitation on top, which materially contributes to overcounting. The new calibration path must treat baseline intensity and excitation separately and identifiably.
- Add explicit diagnostics that report:
  - observed open-session tick count and observation duration
  - fitted baseline intensities and expected event counts over the simulation horizon
  - fitted excitation ratios and stability diagnostics
  - model-implied versus observed comparison metrics used by the objective
  - warnings when the fitted model implies event volumes materially larger than the source data regime
- Validate the calibrated Hawkes model against real-data event volume before it is handed to coarse or seeded simulation. A fitted model that implies multi-order event-count blow-up relative to the source regime should fail diagnostics or be clearly flagged before simulation starts.
- Ensure the manual EUR/USD example and any public market-data workflow use the repaired Hawkes fit path rather than the current one-candidate heuristic path.

### Acceptance Criteria
- The Hawkes stage no longer consists of one heuristic candidate scored by a weighted sum of raw observed summaries.
- The Hawkes objective is based on candidate-dependent model implications rather than candidate-independent observed scores.
- The primary real-data calibration workflow supports unsigned market data and documents how directional asymmetry proxies are estimated.
- Calibration artifacts and diagnostics expose enough information to explain why a Hawkes fit was selected and whether it implies realistic event counts.
- On a representative real-data EUR/USD calibration run, the fitted event regime is materially closer to the observed source-data regime and does not explode to the previously observed approximately `16` million events over `96` simulated hours without an explicit diagnostic failure.
- Public examples and workflow documentation stop implying that the current heuristic Hawkes seed is a fitted model.

### Unit tests
- Tests for unsigned directional-label estimation policies and their diagnostics.
- Hawkes objective tests showing that changing candidate parameters changes the objective through model-implied fit terms.
- Regression tests asserting that the Hawkes stage produces more than one meaningful candidate or otherwise performs a true parameter-search workflow.
- Diagnostics tests asserting that unrealistic implied event counts are surfaced in artifacts or failure reports.
- Real-data regression fixtures comparing observed activity summaries to fitted-model implied summaries within documented tolerances.

### Notes/Constraints
- Do not revert to a requirement for true signed event tapes. That is not a viable product assumption for this workflow.
- The goal is realistic event-rate and asymmetry calibration for unsigned data, not perfect reconstruction of an exchange-grade signed event tape.
- Coordinate with the return-components issue so the chosen asymmetry proxy and event-rate regime are compatible with downstream return-skew calibration rather than fighting it.
