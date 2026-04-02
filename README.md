# Janiform

Janiform is a Python package and CLI for fitting, simulating, pricing, and validating a hyperrealistic high-frequency return model backed by a Rust workspace. The public entrypoints are the `janiform` command-line interface and the importable Python modules under `python/janiform/`.

Janiform is pre-v0.1. Install it from a source checkout with the local bootstrap scripts.

## Why The Name

Janiform takes its name from Janus, the Roman god traditionally depicted with two faces and associated with transitions and thresholds. The model is built around paired and asymmetric structure, including buy and sell excitation and the contrast between open-market moves and closed-market gaps. The `form` part points to the model shape rather than to any one workflow.

## Quickstart

### Linux and WSL

```bash
./scripts/bootstrap-linux.sh .venv-linux
source .venv-linux/bin/activate
janiform --help
python -m examples.introductory.first_model
```

### Windows PowerShell

```powershell
./scripts/bootstrap-win.ps1 .venv
. .\.venv\Scripts\Activate.ps1
janiform --help
python -m examples.high_level.synthetic_eurusd_plot
```

## Fastest Python Path

```python
from pathlib import Path

from janiform.calibration import JaniformEstimator

model = JaniformEstimator(market_profile="eurusd_spot").fit(
    tick_data_path=Path("data") / "Synthetic_EURUSD_Data_1M_ticks.parquet",
)
simulation = model.simulate(
    seed=17,
    start_time_utc="2023-03-28T19:00:00Z",
    horizon_interval_count=24,
)
frame = simulation.to_pandas()
```

Use [docs/getting-started/first-model.md](docs/getting-started/first-model.md) for the full happy-path workflow and pricing notes.
Use [docs/getting-started/concepts.md](docs/getting-started/concepts.md) when terms like market profile, seeded ticks, or anchor are unfamiliar.
Use [examples/README.md](examples/README.md) for the grouped example index.

## Fastest CLI Path

```bash
janiform fit --help
janiform simulate --help
janiform price --help
janiform presets list --help
```

Use [docs/guides/cli-workflows.md](docs/guides/cli-workflows.md) for workflow selection and [docs/reference/cli.md](docs/reference/cli.md) for the curated command reference.

## Documentation

- [docs/README.md](docs/README.md)
- [docs/introduction.md](docs/introduction.md)
- [docs/getting-started/installation.md](docs/getting-started/installation.md)
- [docs/getting-started/quickstart.md](docs/getting-started/quickstart.md)
- [docs/getting-started/concepts.md](docs/getting-started/concepts.md)
- [docs/reference/python-api.md](docs/reference/python-api.md)
- [docs/reference/cli.md](docs/reference/cli.md)
- [CONTRIBUTING.md](CONTRIBUTING.md)

## License

Janiform is licensed under `AGPL-3.0-only`. See [LICENSE](LICENSE). Release artifacts that bundle the native extension also ship [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) for the bundled Rust dependency closure.
