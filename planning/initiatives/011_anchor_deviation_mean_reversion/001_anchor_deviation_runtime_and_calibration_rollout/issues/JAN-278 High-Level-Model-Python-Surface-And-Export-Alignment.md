## High Level Model Python Surface And Export Alignment

### Description
Update the high-level Python model, runtime preparation helpers, pybind transport surface, and export pipeline so the new observed-state layer is available end to end.

This issue is where the fitted model becomes practically usable after the lower-level runtime work lands. The high-level model must be able to:
- persist the mean-reversion parameter block
- materialize interval anchors for requested runtime intervals
- pass mean-reversion inputs into seeded and coarse execution
- export observed price paths correctly

The current high-level export flow reconstructs prices from `reference_price * exp(cumulative_log_return)`. That is no longer correct once observed price is:

```latex
\[
X_t = A_t + D_t.
\]
```

The export path must instead use the observed-state fields emitted by the runtimes.

### Required high-level behavior
- `JaniformModel` persistence must round-trip the mean-reversion parameter block.
- Runtime preparation must materialize interval anchors into the shared runtime-anchor contract introduced earlier.
- High-level `simulate(...)` must pass observed-state boundary input and anchor materialization into both seeded and coarse workflows.
- High-level tabular export and parquet export must use observed price, not reconstructed core cumulative return.

### Required contract behavior
If mean reversion is disabled, high-level public behavior must remain the nested original model after in-repo payload migration.

If mean reversion is enabled, high-level outputs must make the following distinction explicit:
- Janiform core contribution fields
- observed log-price fields
- observed price fields derived from observed log-price

### Files and modules that should be updated
- [`model.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/fit/model.py)
- [`_workflow_outputs.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/_workflow_outputs.py)
- Python simulation package exports
- Pybind simulation transport columns under `crates/janiform-pybind/`
- High-level examples and contract snapshots

### Acceptance Criteria
- A persisted high-level model round-trips the mean-reversion parameter block and any required anchor metadata.
- High-level runtime preparation materializes anchors once and passes them through the canonical seeded and coarse request boundaries.
- Exported pandas, numpy, and parquet simulation outputs use the observed-state path and no longer assume observed price is `exp(core cumulative return)`.
- Public Python contract snapshots and surface-map tests pass with the new fields.
- High-level nested behavior remains correct when mean reversion is disabled.

### Unit tests
- High-level model save/load round-trip including mean-reversion parameters.
- Simulation dispatch tests showing seeded and coarse requests receive the mean-reversion runtime inputs.
- Export tests verifying observed prices come from runtime observed-state fields rather than reconstructed core cumulative return.
- Pybind transport tests for any new observed-state columns or payload fields.

### Notes/Constraints
- Do not duplicate anchor materialization logic in both high-level Python and lower-level runtimes. Materialize once at the high-level boundary and pass explicit aligned values down.
- Keep the high-level surface explicit. The user-facing model should not silently invent anchor defaults beyond the persisted fitted model contract.
