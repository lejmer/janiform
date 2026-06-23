# Introduction

## What Janiform does

Janiform is a realistic high-frequency return model for market prices. It is designed to model observed market behavior well enough to allow you to simulate ultra-realistic price paths, and carry the fitted structure into derivative pricing.

Janiform uses a two-sided structure: buy-side and sell-side forces are modeled as separate but connected parts of price formation. High-frequency markets are shaped by the timing, direction, and intensity of market activity, not only by the sequence of observed price changes.

## Why Janiform is different

Markets do not move at a steady pace. Quiet periods and active periods tend to cluster: when trading pressure builds, the next move is often more likely to arrive soon, and the effect can persist before fading.

Janiform models that activity directly. It separates buy-side and sell-side pressure, lets each side influence future activity, and turns that evolving market activity into price movement. This is how Janiform represents volatility clustering: not as a separate overlay, but as a consequence of clustered market activity.

The fitted model can then be used to simulate price paths, price derivatives, and check model behavior because those outputs all come from the same activity-driven return structure.

In practical terms, Janiform tracks activity states for buy and sell pressure. Those states rise when events arrive, decay over time, and determine how much business time passes for the return drivers. Active markets therefore produce larger and more frequent price variation, while quiet markets produce less.

> For the mathematically curious: Janiform is built to be time-scale consistent. The same fitted model can generate tick-level paths and coarser interval paths without switching to a different model. That matters because high-frequency behavior and aggregated behavior should agree with each other, not come from separate approximations.

## Why the name

Janiform takes its name from Janus, the Roman god traditionally depicted with two faces and associated with transitions and thresholds. The name fits the model's paired and asymmetric structure, including buy and sell activity and the contrast between open-market moves and closed-market gaps.

## What a Janiform model is

A Janiform model is a compact description of how prices can move. It is not a copy of the market data. It is a fitted object that captures structure in the data: how price changes behave, how buy and sell activity appear, and how that behavior carries into simulation and pricing.

You do not need to understand the mathematical details behind Janiform in order to run it. All you need to know is that the model is the thing you fit from data, simulate from, and price with.

## What you put in

Janiform usually starts from market data. Depending on the workflow, that can mean observed price ticks, trade sides, prepared model artifacts, option data, or a preset that describes a market setup.

Presets are there to make market common setups easier to start from. They do not remove the need for market data, but they reduce the amount of configuration you need before a first run.

## What you get out

Janiform produces artifacts you can inspect, save, and reuse:

- fitted models
- simulated price paths
- derivative pricing results
- checks of fitted model behavior
- run outputs for automation

The exact output depends on the workflow. A Janiform run should leave you with something concrete: a fitted model, simulated prices, derivative values, model checks, or a saved run directory.

## What Janiform is for

Janiform is useful when historical data alone is too narrow, and simple simulated returns aren't enough.

Janiform can create realistic market paths for strategy testing, execution testing, risk checks, and system development. A backtest on one historical period only shows one version of the market. Janiform lets you generate many plausible paths where quiet periods, active bursts, market pressure, jumps, and gaps can appear in varied yet very realistic sequences.

Janiform also fits option-pricing workflows that need to reflect the way markets actually move. Clustered activity, jumps, gaps, and buy/sell pressure can change the distribution of future prices. Janiform carries those fitted dynamics into derivative pricing, so pricing experiments can be tied to the same market behavior used for simulation.

Janiform is especially useful for:

- testing trading strategies across many plausible market paths
- stress testing execution, risk, margin, and monitoring systems
- generating synthetic market data for development and automation
- studying how clustered activity changes derivative values
- comparing model-implied derivative prices with observed market prices
- exploring scenarios around events, market regimes, and time horizons

## CLI or Python package?

The CLI runs Janiform from a terminal, shell script, scheduler, or automation system.

The Python library keeps Janiform directly available inside analysis code, notebooks, research scripts, and larger applications.

Both interfaces can run the same Janiform workflows and both can work with saved artifacts. The workflow stays the same across both interfaces.

## Next steps

- [First model](first-model.md)
- [Concepts](concepts.md)
