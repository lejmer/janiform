# Repository Layout

| Path               | Purpose                                                          |
|--------------------|------------------------------------------------------------------|
| `crates/`          | Rust workspace crates                                            |
| `python/janiform/` | Public Python package and CLI                                    |
| `tests/`           | Python tests, fixtures, and support utilities                    |
| `benches/`         | Benchmark sources and related tooling                            |
| `docs/`            | User-facing documentation                                        |
| `examples/`        | Runnable examples                                                |
| `scripts/`         | Bootstrap, build, test, CI, and generation scripts               |
| `tools/`           | Internal contributor tooling                                     |
| `data/`            | Checked-in sample and development data                           |
| `presets/`         | Bundled preset artifacts                                         |

## Notes

- Keep user-facing documentation in `docs/`.
- Keep local `README.md` files focused on their own directory rather than duplicating top-level docs.
- Do not edit machine generated local `README.md` files as they will be overwritten.
