# Workflow Examples

These examples exercise the current public Python workflow contracts directly.
They import only public Janiform modules and are designed to be small enough to
read end-to-end.

## Synthetic EUR/USD

Run:

```bash
PYTHONPATH=python python3 -m examples.synthetic_eurusd
```

This a very short-and-to-the-point end-to-end example. It:
- loads the checked-in synthetic EUR/USD parquet fixture
- fits a `JaniformModel` with `JaniformEstimator`
- runs both coarse-interval and seeded-tick simulations from the fitted model
- opens a two-panel plot so you can inspect the simulated paths visually

This example uses the bundled dataset at
`data/Synthetic_EURUSD_Data_1M_ticks.parquet`, so you can run it without
preparing any separate input files first.
