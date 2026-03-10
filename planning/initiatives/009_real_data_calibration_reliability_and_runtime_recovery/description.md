## Initiative 009: Real-Data Calibration Reliability And Runtime Recovery

Recover the market-data calibration and seeded-simulation workflow so it behaves credibly on real unsigned broker tick data, produces realistic scale on both coarse and seeded paths, avoids unacceptable runtime and memory blowups, and removes manual session-calendar administration from common workflows.

### Desired outcome
- Replace heuristic or weakly identified real-data calibration paths with calibration workflows that are actually constrained by observed unsigned tick data.
- Bring coarse and seeded simulations back into the same realistic event-count and return-scale regime as the source data when using real EUR/USD style broker ticks.
- Remove full-fine-path JSON round-tripping and other avoidable boundary copies that currently make seeded simulation too slow and memory-hungry for practical use.
- Infer or materialize session behavior from the data and workflow context so users do not need to hand-author exchange calendars for common broker datasets.
- Produce implementation issues that are self-contained enough for an engineer to execute without needing any conversational context from the investigation that motivated this initiative.

### Success signals
- A representative real-data smoke test on the existing EUR/USD dataset no longer calibrates a model that explodes from roughly 664k observed ticks over several days to roughly 16 million generated events in a 96-hour simulation horizon.
- Coarse and seeded simulated prices remain in a realistic scale regime relative to the observed input path instead of diverging into ranges such as approximately `0.25` to `2.00` when the observed series remains near approximately `1.075` to `1.09`.
- Seeded simulation no longer spends most of its time and memory materializing duplicate fine-path payloads across Rust, PyO3, JSON, and Python object graphs.
- Common FX and CFD calibration workflows can run without manually constructing a session definition file, while still exposing diagnostics and explicit overrides when inference is uncertain.
- New project and issue plans explicitly document the unsigned-data requirement and do not rely on the earlier assumption that true signed trade event tapes are available.

### Example projects that might be useful to add
- A project focused on real-data benchmark fixtures and acceptance baselines for calibration, event counts, wall-clock time, and peak memory.
- A project focused on session-inference validation across FX, CFD, and equities-style datasets.
- A project focused on calibration diagnostics and explainability artifacts so poor fits are visible before simulation is launched.
