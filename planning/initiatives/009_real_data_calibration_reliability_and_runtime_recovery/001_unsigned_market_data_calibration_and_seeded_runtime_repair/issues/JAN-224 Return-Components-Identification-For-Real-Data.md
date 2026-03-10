## Return Components Identification For Real Data

### Description
Rework return-components calibration so NIG scale, skew control, jump scales, gap behavior, and news-to-activity coupling are identified from enough real-data evidence to produce realistic return scale on observed unsigned broker tick data.

The real-data smoke test showed that the scale problem is too large to be explained by closed-session activity alone. The observed EUR/USD path remained roughly in the range `1.075` to `1.09`, while the coarse and seeded simulated paths wandered roughly between `0.25` and `2.00`. Closed-session handling amplified the mismatch, but it did not create that scale error by itself.

The current return-components fit is weakly identified:
- In `python/janiform/calibration/_market_data_translation.py`, `_derive_intraday_jump_candidates` keeps only material returns above a threshold and falls back to the single largest candidate if none exceed the threshold.
- `_derive_nig_candidates` returns exactly one NIG shared-plus-shift configuration from crude moments such as mean return, RMS return, sell-share proxy, mean open tick count per hour, and average option quote level.
- In the investigated EUR/USD run, the diagnostics reported `intraday_jump_candidate_count = 1`, `material_close_open_gap_count = 0`, and the return-components artifact still selected nontrivial activity-coupling and jump scales from that sparse evidence.
- In `crates/core/src/calibration/stages/return_components.rs`, candidate families are built from only a few summary statistics, with tiny multiplicative families around surprise intensity, activity coupling, and mark scales.
- The stage objective is then scored from relative error to those summary statistics rather than from a richer fit to observed return distributions or cross-scale behavior.

That structure is not robust enough to identify realistic return scale on noisy real data. It is especially weak when gaps are rare, known-news anchors are sparse, or the input is unsigned market data where asymmetry must be inferred indirectly.

### Implementation details
- Redesign return-components identification so it uses richer observed evidence than one NIG seed plus a tiny multiplicative family grid.
- Separate the roles of:
  - continuous trading-time return scale and skew
  - intraday jump detection and jump magnitude calibration
  - close-open gap behavior
  - news arrival intensity and activity-coupling calibration
- Replace the current one-candidate NIG path with a true search or estimation workflow that can fit NIG parameters against observed open-session return behavior. The fit should be grounded in realized return scale, skewness, kurtosis, and other documented statistics rather than only a small set of summary moments.
- Revisit jump and gap candidate generation. The current "largest candidate if none exceed threshold" rule can create false structure from sparse data. Materiality rules and fallback behavior should be redesigned so the stage distinguishes "insufficient evidence for jumps or gaps" from "one large event implies a calibrated jump process."
- Ensure the stage can identify scale on data where close-open gaps are absent or rare. The absence of material gaps must not leave the whole return model underdetermined.
- Add diagnostics that compare observed and fitted values for:
  - return volatility
  - mean return drift over the calibration window
  - skewness
  - excess kurtosis
  - jump-frequency and jump-size diagnostics
  - gap-frequency and gap-size diagnostics when relevant
  - activity-lift behavior around detected shocks
- Add a calibrated-model sanity check that flags implausible price-range expansion before simulation is accepted. The smoke test demonstrated that a parameter set can pass the current stage while producing price paths that are multiple orders of magnitude wider than observed reality.
- Ensure the calibrated return-components artifact is sufficiently documented that an implementer can tell whether the dominant scale error came from NIG scale, jump scales, activity-coupling news marks, or gap behavior.

### Acceptance Criteria
- The return-components stage no longer depends on a single NIG candidate and a tiny family grid derived from a handful of summary statistics.
- The stage distinguishes "insufficient evidence" from "identified component" for intraday jumps and close-open gaps instead of forcing one large return into a jump model by default.
- Calibration diagnostics expose observed versus fitted return-scale and shape metrics, including volatility, skewness, and kurtosis.
- The selected return-components fit can be traced back to explicit evidence in the data rather than undocumented heuristics.
- On representative real-data EUR/USD smoke tests, calibrated coarse and seeded paths remain in a realistic scale regime relative to the observed path and no longer produce the previously observed extreme price-range blow-up without an explicit diagnostic failure.
- The implementation is compatible with unsigned market-data workflows and does not rely on true signed event tapes.

### Unit tests
- Regression tests for NIG calibration showing multiple candidate regions or a true fitting loop rather than one heuristic candidate.
- Tests for jump and gap candidate generation that distinguish sparse evidence from genuine repeated structure.
- Diagnostics tests for observed-versus-fitted return-shape metrics.
- Real-data regression fixtures that assert calibrated return volatility and higher-moment diagnostics stay within documented tolerances on representative unsigned datasets.
- Cross-scale regression tests showing that improved return-components calibration reduces the observed coarse-versus-seeded and observed-versus-simulated scale mismatch.

### Notes/Constraints
- Closed-session handling remains relevant, but this issue must not treat it as the sole explanation for the return-scale failure.
- Coordinate with the Hawkes issue, because event-count blow-up and return-scale blow-up are coupled. Better return-components identification alone will not fully solve the problem if Hawkes still overproduces events by an order of magnitude.
- Keep the workflow focused on real unsigned broker tick data, not exchange-only data products with richer labels that are unavailable to this user base.
