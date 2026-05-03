# Janiform

[![CI](https://github.com/lejmer/janiform/actions/workflows/ci-fast-regression.yml/badge.svg?branch=develop)](https://github.com/lejmer/janiform/actions/workflows/ci-fast-regression.yml)
[![Tests](https://github.com/lejmer/janiform/actions/workflows/ci-pr-test-rust.yml/badge.svg?branch=develop)](https://github.com/lejmer/janiform/actions/workflows/ci-pr-test-rust.yml)
[![License](https://img.shields.io/badge/license-AGPL--3.0--only-blue.svg)](https://github.com/lejmer/janiform/blob/develop/LICENSE)
[![Python](https://img.shields.io/badge/python-3.12%2B-blue)](https://github.com/lejmer/janiform/blob/develop/pyproject.toml)
[![Rust](https://img.shields.io/badge/rust-1.89%2B-orange)](https://github.com/lejmer/janiform/blob/develop/Cargo.toml)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows-lightgrey)](https://github.com/lejmer/janiform)

A high-frequency return model written in Rust, capable of hyperrealistic simulation of market prices and derivatives pricing. Janiform comes both as a command-line interface and an importable Python package.

> **NOTE:** Janiform is pre-v0.1. Install it from a source checkout with the local bootstrap scripts.

## Why the name

Janiform is named after Janus, the Roman god of duality, transitions, and thresholds. The name reflects the model's paired structure, natively incorporating buy and sell forces. The "form" part refers to the mathematical form of the model itself. Janiform is a two-sided functional form with invariant structure under transformation, designed to stay coherent across simulation, aggregation, and pricing.

## Quickstart

### Linux

```bash
./scripts/bootstrap-linux.sh .venv
source .venv/bin/activate
janiform --help
python -m examples.introductory.first_model
```

### Windows

```powershell
./scripts/bootstrap-win.ps1 .venv
. .\.venv\Scripts\Activate.ps1
janiform --help
python -m examples.introductory.first_model
```

## Fastest Python path

```python
from pathlib import Path

import pandas as pd

from janiform.calibration import JaniformEstimator
from janiform.model import JaniformModel
from janiform.simulation import JaniformSimulationResult

estimator = JaniformEstimator()
model: JaniformModel = estimator.fit(tick_data_path=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet")
simulation: JaniformSimulationResult = model.simulate(horizon_interval_count=24)

df: pd.DataFrame = simulation.to_pandas()
print(df.head())
```

Use [docs/getting-started/first-model.md](docs/getting-started/first-model.md) for the full happy-path workflow and pricing notes.
Use [docs/getting-started/concepts.md](docs/getting-started/concepts.md) when terms like market profile, seeded ticks, or anchor are unfamiliar.
Use [examples/README.md](examples/README.md) for the grouped example index.

## Fastest CLI path

```bash
janiform fit --help
janiform simulate --help
janiform price --help
janiform inspect --help
janiform report --help
janiform presets list --help
```

Use [docs/interfaces/cli.md](docs/interfaces/cli.md) for CLI mechanics and the workflow pages for task-focused examples.

## Documentation

- [docs/README.md](docs/README.md)
- [docs/getting-started/installation.md](docs/getting-started/installation.md)
- [docs/getting-started/concepts.md](docs/getting-started/concepts.md)
- [docs/getting-started/first-model.md](docs/getting-started/first-model.md)
- [docs/workflows/fit-models.md](docs/workflows/fit-models.md)
- [docs/workflows/simulate-paths.md](docs/workflows/simulate-paths.md)
- [docs/workflows/price-derivatives.md](docs/workflows/price-derivatives.md)
- [docs/workflows/validate-models.md](docs/workflows/model-validation.md)
- [docs/interfaces/cli.md](docs/interfaces/cli.md)
- [docs/interfaces/python.md](docs/interfaces/python.md)
- [docs/reference/python-api.md](docs/reference/python-api.md)
- [docs/reference/cli.md](docs/reference/cli.md)
- [CONTRIBUTING.md](CONTRIBUTING.md)

## License

Janiform is licensed under `AGPL-3.0-only`. See [LICENSE](LICENSE). Release artifacts that bundle the native extension also ship [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) for the bundled Rust dependency closure.
