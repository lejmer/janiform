## Seeded Fine Price Path And Signed Calibration Alignment

### Description
Bring the canonical fine-scale Janiform implementation into line with the design documents by making seeded simulation produce a priced fine path rather than only an event tape, by using the same return-decomposition and news contracts as the coarse engine, by requiring or explicitly modeling signed trade inputs for design-faithful Hawkes calibration, and by extending scale-consistency validation to compare returns instead of only activity and jump-routing diagnostics.

Today the coarse engine matches the design more closely than the seeded engine. Coarse simulation already consumes the buy-side and sell-side NIG return decomposition and emits return components. The seeded simulator does not consume the NIG return configuration, does not emit a public price or return path, uses simplified seeded-only news mark defaults instead of the full calibrated news-process configuration, and is therefore unable to realize the design equation
`X_t = L^+_{A^+_t} - L^-_{A^-_t} + M_t + G_t`
at the canonical fine scale.

The design note is explicit that the canonical fine-scale object is the bivariate buy and sell point process with stochastic intensity and exogenous shocks, and that the two Lévy drivers are the decomposition of log returns rather than a separate consumer-only coarse artifact. It is also explicit that Hawkes calibration is meant to use trade timestamps with buy and sell sign labels, and that scale consistency should mean coarse returns remain aligned with aggregation of the fine-scale model structure. This issue closes the implementation gap so someone can calibrate, simulate, and validate Janiform against the design without relying on internal knowledge or conversation context.

### Implementation details
- Replace the current event-tape-only seeded simulation contract with one canonical fine-path contract that includes priced return realization. The canonical seeded output should expose, at minimum, ordered transition timestamps, event type, buy or sell side when applicable, cumulative log price or log-return level, and component breakdown fields for `buy_side_return`, `sell_side_return`, `trading_time_return`, `open_jump_return`, `closed_gap_return`, and `total_return`. If an event-tape view is still useful, make it a derived projection of the priced fine path rather than the primary contract.
- Extend seeded simulation model inputs so the fine engine consumes the same calibrated return-decomposition inputs as the coarse engine. The seeded run config should accept the NIG shared-plus-shift configuration or the already-derived buy and sell driver parameters, not only Hawkes parameters and hourly factors.
- Refactor seeded runtime so trading-time returns are realized on the fine path. The implementation should sample buy-side and sell-side NIG increments over each fine transition slice using the integrated business-time increments implied by the seeded Hawkes path between successive transitions. The fine engine must no longer stop at reconstructing `A^+`, `A^-`, `N^+`, and `N^-` for diagnostics. It must actually turn those fine-scale business-time increments into realized return increments and accumulate them into a public log-price path.
- Materialize `M_t` and `G_t` on the public fine path rather than only in debug traces. Open-window news jumps must appear on the first post-arrival open priced transition. Closed-window news jumps must appear on the first reopening priced transition as a gap. Debug traces can continue to expose routing internals, but the user-facing fine-path artifact must show the realized priced effect.
- Remove seeded-only neutral news mark defaults as the model-consistent path. Seeded simulation should consume the same calibrated `NewsProcessConfiguration` semantics as coarse simulation, including known-news anchors, surprise-news intensity controls, shared Gamma and Kou style mark law parameters, and component mark scales. If any simplified seeded-only path is retained for test fixtures, it must be an explicitly named non-canonical test mode rather than the default behavior.
- Unify seeded and coarse news semantics at the contract boundary. The same domain model should drive known-news arrivals, surprise-news arrivals, activity-coupling marks, open-window direct jumps, and closed-window gaps. Avoid duplicating separate seeded-only sampling logic when the shared `news` domain can be reused.
- Update market-data calibration so the design-faithful Hawkes path uses signed trade events. The calibration workflow should accept trade timestamps with buy and sell sign labels as canonical Hawkes inputs. The current midpoint-tick-only translation path should not remain the canonical Janiform calibration workflow for the full model. Either:
  1. change the primary market-data calibration contract to require signed trade events for full Hawkes calibration, or
  2. split the current midpoint-based workflow into an explicitly approximate or reduced-information path that is documented as not fully design-faithful.
- Remove implicit heuristic Hawkes seeding from raw tick timestamps as the only full-model path. If midpoint-only data is still supported for some workflow, its scope must be explicit and separate from full signed-trade calibration so users cannot mistake it for the canonical model-fitting path described in the design note.
- Update staged calibration artifacts and reload plumbing so the seeded simulator can be rehydrated from the same fitted return-components and news-process artifacts already used by the coarse engine. A user should be able to calibrate once and then run both coarse and seeded priced simulations from the fitted artifacts without manual reconstruction gaps.
- Extend coarse-versus-fine consistency validation beyond activity and routing checks. The consistency harness should compare fine-aggregated and direct-coarse `trading_time_return`, `open_jump_return`, `closed_gap_return`, and `total_return`, with explicit expectation classes for exact, approximate, and distributional checks. Existing activity and state-carryover checks should remain, but they are not sufficient on their own.
- Update public Python contracts, CLI surfaces, examples, docs, and validation fixtures to reflect the new canonical fine-path semantics. The end-to-end examples must stop describing seeded simulation as only an event tape once the design-faithful path exists.

### Acceptance Criteria
- Seeded simulation has one canonical public fine-path output that contains priced return realization rather than only timestamp and side events.
- Seeded simulation consumes calibrated NIG buy and sell return parameters and calibrated news-process configuration rather than seeded-only simplified defaults.
- Fine-path outputs include realized `buy_side_return`, `sell_side_return`, `trading_time_return`, `open_jump_return`, `closed_gap_return`, and `total_return` at a documented transition granularity.
- Open-window news and closed-window news are reflected in the public fine-path price evolution with the same routing semantics used by the shared-domain news contracts.
- Full-model market-data calibration defines a canonical signed-trade input path for Hawkes fitting that matches the design requirement for buy and sell labeled trade arrivals.
- If midpoint-only calibration remains available, it is explicitly documented and contractually separated as an approximate or reduced-information workflow rather than the canonical design-faithful path.
- Staged calibration outputs are sufficient to run both coarse simulation and seeded priced simulation without ad hoc reconstruction outside the documented artifacts.
- Cross-scale validation compares fine-aggregated and direct-coarse return components in addition to activity statistics, jump-routing presence, and state carryover.
- Repository docs and user-facing examples describe the fine-scale model as a priced path and no longer imply that the canonical seeded output is only an event tape.

### Unit tests
- Contract tests for the new seeded fine-path artifact schema, including required priced fields and component breakdown completeness.
- Deterministic Rust and Python parity tests for seeded priced-path generation under fixed seeds.
- Regression tests showing seeded fine-path cumulative returns change when NIG driver parameters change, proving the fine engine is no longer Hawkes-only.
- Regression tests for open-window news realization on the first post-arrival open priced transition and closed-window news realization on the first reopening gap transition.
- Cross-engine tests showing seeded and coarse simulation consume the same calibrated news-process semantics and produce compatible direct-jump behavior.
- Calibration contract tests for the canonical signed-trade Hawkes input path, including validation failures for missing sign labels when the full-model path is requested.
- Workflow tests covering calibrated artifact reload into both coarse and seeded priced simulation entrypoints.
- Scale-consistency regression tests comparing fine-aggregated and direct-coarse return-component distributions and cumulative totals, not only `A`, `N`, and routing diagnostics.
- Example and CLI smoke tests updated for the new seeded priced-path terminology and payloads.

### Notes/Constraints
- Pre-v0.1 policy applies. Backward compatibility is not required and should not drive the design. Replace incorrect canonical contracts directly and migrate in-repo call sites, tests, fixtures, docs, and examples in the same change series.
- Do not bump schema versions during this work. Keep schema identifiers at `v1` while replacing old contracts and fixtures.
- Reuse shared-domain helpers from `news`, `risk`, `return_decomposition`, and simulation modules instead of adding a parallel seeded-only pricing stack.
- Keep deterministic seeding and reproducibility guarantees intact across Rust, PyO3, Python, fixtures, and examples.
- Coordinate with existing seeded-alignment and consistency issues in this project so scheduling, news semantics, priced fine-path realization, and validation updates converge on one canonical model rather than another temporary compatibility layer.
