## Initiative 006: Python Surface And v0.1 Release Readiness

Ship a production-ready Python and CLI surface with explicit contracts, hardened bindings, and release-quality process gates, with special focus on market-professional workflows that start from real market data files instead of internal execution contracts.

### Desired outcome
- Janiform ships a usable Python package and CLI backed by the Rust core, with clear boundaries between `core` and `pybind`.
- The v0.1 developer and user experience is documented well enough for internal teams and potential open-source adopters, including a calibration path that feels natural to market users.
- The repository has reliable quality gates for packaging, typing, tests, and build verification.

### Success signals
- Python API and CLI expose simulation, calibration, and pricing workflows with explicit user-facing contracts aligned to the corrected news model semantics.
- Calibration users can supply session definitions, known-news calendars, and `csv` or `parquet` market data sources without needing to author staged execution payloads by hand.
- The staged calibration execution contract remains available as an advanced or internal boundary, while the default user-facing surface is market-data-first.
- Execution-mode defaults are explicit and enforced: CLI defaults to `execution_mode=disk`, Python importable API defaults to `execution_mode=memory`, and both surfaces support explicit override.
- PyO3 boundary code is thin and consistent, with explicit conversion and error mapping.
- CI-level checks pass for core build, Python importability, repository verification commands, and at least one representative file-driven calibration workflow.
- Documentation covers install, quickstart, model concepts, and examples for the primary v0.1 workflows, especially calibration from real market data files.
