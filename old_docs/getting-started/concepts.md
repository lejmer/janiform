# Core Concepts

These are the terms that show up most often in Janiform workflows.

## Market Profile

A market profile is a named bundle of market-specific defaults. It can carry a session baseline, a known-news reference, and other inputs Janiform needs to turn raw data into a runnable workflow. Choosing `eurusd_spot` tells Janiform which market conventions to start from before you override anything explicitly.

## Session

A session describes when the market is open, closed, or transitioning between those states. Janiform uses sessions to decide how timestamps are grouped, how overnight gaps are handled, and how hourly factors line up with the market clock.

## Known News

Known news is scheduled event information supplied ahead of time, such as macro releases or other timestamped announcements. Janiform uses it as explicit input instead of trying to infer every jump from prices alone. It matters both during calibration and in workflows that distinguish open-market jumps from closed-market gaps.

## Reference Price

Janiform stores many quantities in log coordinates relative to a reference price. If the reference price is `1.0800`, an observed price is converted to `log(price / 1.0800)`. This keeps the model in a stable coordinate system while still letting results be reported back in price units.

## Mean Reversion Anchor

When mean reversion is enabled, Janiform separates the current log price into an anchor and a deviation from that anchor. The anchor is the latent level the process is organized around. The deviation is how far the observed log price sits from that level. The anchor can move over time, so mean reversion is not just a pull back to one fixed constant.

## Hourly Factors

Hourly factors are interval-aligned multipliers that encode time-of-day structure. The seasonality factor scales baseline activity by hour. News-intensity factors describe how much scheduled news should raise expected activity in each interval. Pricing, simulation, and calibration all consume the same interval-aligned factors.

## Seeded-Tick Simulation

Seeded-tick simulation produces a fine-grained event path from a model and a random seed. Use it when you want replayable discrete-event structure rather than only interval summaries.

## Coarse-Interval Simulation

Coarse-interval simulation generates interval returns directly without replaying every fine-scale event. Use it when the interval path is what matters and you do not need a synthetic tick tape.

## Staged Calibration

Staged calibration fits the model in ordered pieces instead of solving everything in one step from the start. Janiform estimates components such as seasonality, self-exciting trade activity, return components, and mean reversion in sequence, then performs joint refinement. This makes the fit procedure easier to inspect, checkpoint, and debug.

## Stylized Facts

Stylized facts are recurring statistical patterns seen in market data, such as heavy tails, clustered activity, or horizon-dependent behavior. Janiform uses stylized-fact workflows to compare generated paths against those patterns instead of checking only point-by-point path similarity.

## Anchored Pricing

Vanilla option pricing uses the fitted model plus the valuation time and spot to recover the current pricing state. The low-level pricing contracts still expose that state explicitly. The high-level API and CLI derive it for you.
