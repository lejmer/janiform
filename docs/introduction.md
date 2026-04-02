# Introduction

Janiform is a model and software stack for high-frequency financial returns. It fits market data, simulates future paths, prices vanilla options, and runs validation workflows from one shared model family.

At a high level, Janiform combines:

- self-exciting buy and sell activity
- heavy-tailed return shocks
- scheduled and unscheduled news effects
- market-open and market-closed behavior
- a mean-reversion layer around an evolving anchor for the observed price level

The Python package gives you the public API and CLI. The Rust workspace runs the numerical engines underneath.

## What Janiform Is For

Janiform exists to cover a gap between two common extremes.

One extreme is a very detailed tick model that can generate realistic event flow but is too expensive or awkward to reuse for pricing and large-scale risk work. The other extreme is a coarse-time return model that is easy to price or simulate but throws away too much of the microstructure that matters for calibration and path realism.

Janiform is built to sit between those extremes. The same fitted model is meant to support:

- realistic high-frequency path generation
- direct interval-level simulation without replaying every tick
- vanilla option pricing
- regression and stylized-fact validation

That is the core purpose behind the project: one model family that stays useful at more than one time scale.

## What The Model Tries To Capture

Janiform is designed around a few recurring properties of market data:

- intraday, weekly, and calendar-driven seasonal structure
- trading activity arrives in bursts rather than on a fixed clock
- buy and sell flow are asymmetric and self-exciting
- returns are heavy-tailed rather than close to Gaussian
- volatility and activity cluster through time
- scheduled news and surprise shocks
- closed-market periods behave differently from open trading windows

That is why the model has explicit buy and sell activity, explicit news inputs, hourly factors, and separate treatment of open-window jumps versus closed-window gaps.

## Why There Are Two Simulation Paths

Janiform supports both seeded-tick simulation and coarse-interval simulation because those workflows serve different purposes.

Seeded-tick simulation is for fine-grained event paths when you want replayable trade-level structure. Coarse-interval simulation is for interval returns when you care about the path at a larger horizon and do not want to pay for a full synthetic tick tape. The project aims to make those two views consistent parts of the same fitted model rather than unrelated tools.

## Why Pricing Belongs Here

Pricing is part of Janiform because the project is not only about generating plausible paths. It is also about making the fitted model usable for option valuation and risk work without forcing a full fine-scale replay every time.

That is why the codebase includes direct pricing and sufficient-statistics risk workflows alongside calibration and simulation.

## Why The Name

Janiform takes its name from Janus, the Roman god traditionally depicted with two faces and associated with transitions and thresholds. The name fits the model's paired and asymmetric structure, including buy and sell activity and the contrast between open-market moves and closed-market gaps.

## Where To Go Next

- Read [getting-started/concepts.md](getting-started/concepts.md) for the main Janiform terms.
- Read [getting-started/quickstart.md](getting-started/quickstart.md) for the shortest working flow.
- Read [getting-started/first-model.md](getting-started/first-model.md) for a complete first fit, simulation, and pricing walkthrough.
