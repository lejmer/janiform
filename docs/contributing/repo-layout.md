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

## Adding A Rust Workspace Crate

Add a crate only when it creates a clear ownership boundary. Before adding one, decide:

- which domain owns the crate and why existing crates should not own the code
- whether the crate is production, boundary, test-only, benchmark-only, facade, or support code
- which internal crates it may depend on without reversing architecture direction
- whether its API is public or `pub(crate)` implementation detail
- whether it must stay independent of Python, terminal rendering, async runtimes, or process globals

When adding the crate:

- add it to the workspace members in `Cargo.toml`
- give `package.description` the exact crate summary that should appear in generated README files
- keep `lib.rs` thin and put declarations in focused modules
- add the crate to `scripts/generate_internal_crate_graph_policy.py` with its classification and layer
- update architecture docs when the crate is important enough to appear in the workspace overview
- regenerate crate README files with `python scripts/generate_crate_readmes.py`
- regenerate the crate graph policy fixture with `python scripts/generate_internal_crate_graph_policy.py`
- run both generator checks before finishing
