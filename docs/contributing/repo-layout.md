# Repository Layout

| Path | Purpose |
| --- | --- |
| `python/janiform/` | Public Python package and CLI |
| `crates/` | Rust workspace crates |
| `tests/` | Python tests, fixtures, and support utilities |
| `data/` | Checked-in sample and development data |
| `examples/` | Runnable examples |
| `scripts/` | Bootstrap, build, test, CI, and generation scripts |
| `tools/` | Internal contributor tooling such as the calibration workbench |
| `docs/` | Human-facing documentation |
| `design/` | Archived theory and design material still needed by the codebase |
| `presets/` | Bundled preset artifacts |
| `benches/` | Benchmark sources and related tooling |

## Notes

- Keep human-facing documentation in `docs/`.
- Keep machine-readable inputs and outputs in fixtures, test data, or dedicated data directories.
- Keep local `README.md` files focused on their own directory rather than duplicating top-level docs.
