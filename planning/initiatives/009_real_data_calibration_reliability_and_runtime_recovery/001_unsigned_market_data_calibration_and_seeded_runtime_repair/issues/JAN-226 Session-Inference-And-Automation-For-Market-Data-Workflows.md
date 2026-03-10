## Session Inference And Automation For Market Data Workflows

### Description
Automate session handling for market-data calibration workflows so users do not need to manually construct exchange calendars or hand-maintain pragmatic session-definition JSON files for common datasets such as FX and CFD broker ticks.

The current manual EUR/USD workflow still depends on a default calendar builder in `examples/eurusd_end_to_end_manual_test.py` that creates a weekday `23:59` close and `00:00` reopen pattern. That default was acceptable as a stopgap but does not satisfy the intended product goal of automation over admin. It also creates two concrete problems:
- It pushes session semantics into user-facing examples instead of deriving them from the workflow context.
- It can inject boundary assumptions that distort observed-versus-simulated comparisons and downstream calibration diagnostics.

The real-data smoke test confirmed that the calendar path is still manual and fragile. Although closed-session drift is not the primary reason for the severe price-scale failure, incorrect or hand-authored session behavior remains an important secondary source of mismatch and operational burden.

### Implementation details
- Design and implement an automatic session-resolution workflow for market-data calibration inputs. The system should infer session behavior from available evidence such as:
  - exchange code or instrument class
  - observed gap structure in timestamps
  - recurring weekly inactivity windows
  - existing registry metadata or broker-market defaults when appropriate
- Define a canonical session-inference artifact or diagnostic report that records:
  - inferred open and closed regime pattern
  - timezone assumption
  - confidence or ambiguity diagnostics
  - reasons for the selected inference
  - whether a manual override was applied
- Ensure common workflows can omit `--session-definition-json` when the system has enough information to infer the session calendar with acceptable confidence.
- Preserve explicit override support for ambiguous or unusual datasets, but make it a documented override path rather than the default expectation.
- Remove hard-coded session construction from user-facing examples once the automated path exists. Examples should demonstrate automation first and explicit override second.
- Integrate session inference with both calibration and simulation preparation so observed data windows, close-open boundary detection, hourly-factor materialization, and seeded routing all consume the same inferred session definition.
- Add diagnostics that distinguish:
  - inferred open-session duration
  - inferred recurring closed windows
  - detected close-open boundaries from the data
  - discrepancies between inferred session behavior and explicit exchange metadata when both exist
- Ensure the inference path fails loudly or requests an override when data is too ambiguous rather than silently inventing a confident but wrong calendar.

### Acceptance Criteria
- A common market-data calibration workflow can run without a manually authored session-definition file when the dataset provides enough evidence for automatic session inference.
- The inferred session behavior is captured in a diagnostic or artifact that explains what was inferred and why.
- Public examples no longer manually build a default FX session definition as the primary workflow.
- Calibration, close-open boundary detection, hourly-factor materialization, and seeded simulation all consume the same session-resolution result.
- Ambiguous datasets are surfaced explicitly with diagnostics and override instructions instead of silently receiving an undocumented default.
- Automated session handling reduces user administration while still supporting explicit manual overrides for special cases.

### Unit tests
- Session-inference regression fixtures for common FX and CFD style weekly trading patterns.
- Tests for timezone inference or timezone-validation behavior where timestamps are provided without explicit timezone metadata.
- Ambiguity tests where multiple session hypotheses fit the data, asserting that the workflow surfaces uncertainty rather than silently choosing one.
- End-to-end workflow tests showing calibration and simulation succeed without manual session-definition input on representative datasets.
- Example or CLI regression tests updated to use the automated session path.

### Notes/Constraints
- Session inference is an automation requirement, not just a convenience feature. The user-facing workflow should not depend on hand-authored calendars for common cases.
- This issue should not overclaim. If reliable inference is impossible for a dataset, the workflow must say so and request an explicit override.
- Coordinate with existing calendar infrastructure under Initiative 001 so the inferred output reuses the canonical session-definition and registry machinery instead of creating a parallel calendar stack.
