# Concepts

Janiform models markets as uncertain, active systems. Prices do not move in a smooth line, and the next movement is never known in advance. A market can be quiet for a while, then suddenly become active. Buy pressure can build. Sell pressure can build. News can arrive. Liquidity can change. The same market can behave differently during the open, near the close, around scheduled events, or overnight.

This page explains the concepts that make those behaviors understandable. You do not need to master the mathematics before using Janiform, but you should know what the main words mean and why they matter.

## Why models are needed

Historical market data shows one realized path: what happened once. It does not show all the other paths that could plausibly have happened.

A model is useful because it turns observed data into a reusable description of behavior. After fitting a model, you can simulate new paths, test systems against more than one historical episode, and study how market dynamics affect derivative prices.

Janiform is built for this job at high frequency. It focuses on how activity arrives, how buy and sell pressure evolve, how returns behave over different time scales, and how those pieces combine into realistic price paths.

## Ticks

A tick is one market observation. In a simple dataset, a tick might contain a timestamp and a price. In a richer dataset, it can also include trade side, bid, ask, or other fields.

Ticks matter because high-frequency markets are not only about prices. The timing of observations is informative. A burst of ticks says the market is active. Long pauses say activity is low. Janiform uses tick data to learn both price behavior and activity behavior.

## Activity

Activity means how much market movement is happening over time. A market with frequent trades, quote updates, or price changes is active. A market with sparse observations is quiet.

Activity is central to Janiform. Instead of treating every clock minute as equally important, Janiform models the market's own pace. Active periods carry more market information than quiet periods.

## Intensity

Intensity is the rate at which market events arrive. If ticks are arriving quickly, intensity is high. If ticks are rare, intensity is low.

In Janiform, intensity is dynamic. It can rise after activity arrives and then decay as the market calms down. This lets the model represent the familiar pattern where active markets often stay active for a while instead of instantly returning to normal.

## Seasonality

Seasonality is a regular pattern that depends on time. Markets often have predictable daily or weekly rhythms. For example, activity can be different near the open, during lunch, near the close, or around regular macroeconomic release times.

Janiform separates regular seasonal structure from more irregular activity. This matters because a busy market at the same time every day is different from a sudden burst of unexpected activity.

## Returns

A return is a price change expressed relative to the starting price. Returns are usually more useful than raw price differences because they can be compared across price levels and time intervals.

Janiform learns return behavior from data and uses that fitted structure when it simulates new paths. The model is interested not only in the size of returns, but also in how returns relate to market activity.

## Volatility

Volatility describes how much prices vary. A low-volatility period has small movements. A high-volatility period has larger movements.

Volatility is not constant in real markets. It changes over time, and those changes are a major reason simple simulations often look unrealistic.

## Volatility clustering

Volatility clustering means large movements tend to appear near other large movements, and quiet periods tend to appear near other quiet periods.

Janiform represents volatility clustering through activity. When market activity clusters, the model's effective market time speeds up. That produces more frequent and larger price variation. When activity is low, market time slows down and price variation becomes quieter.

This is one of the key ideas behind Janiform: clustered volatility is not bolted on as a separate adjustment. It comes from the fitted activity dynamics.

## Buy and sell forces

Markets have two sides. Buy-side pressure and sell-side pressure can behave differently, and both can affect future activity and price movement.

Janiform models buy and sell forces separately. A buy event can increase future buy-side activity. A sell event can increase future sell-side activity. The two sides can also interact. This gives the model a richer view of market pressure than a one-sided activity count.

## Price paths

A price path is a sequence of prices through time. Historical data contains observed paths. Simulation produces new paths from a fitted model.

The point of a simulated path is not to predict the exact future. It is to generate a plausible future. A useful simulated path should have realistic timing, activity, price variation, jumps, and quiet periods.

## News, jumps, and gaps

News can move markets abruptly. Sometimes the event is known in advance, such as a scheduled economic release. Sometimes the shock is unexpected, such as breaking news or unexplained order-flow pressure.

A jump is a sudden price move. A gap is a discontinuity between one observed price and the next, often because the market was closed, liquidity was thin, or important information arrived between observations.

Janiform can work with known news inputs when they are available. Known news helps the model distinguish scheduled event behavior from ordinary market activity. Unexpected shocks are the remaining abrupt moves that are not tied to known events in the input data.

## Market sessions

A market session describes when a market is open, when it is closed, and which timezone those rules use.

Sessions matter because a price move during an active trading session is not the same as a move across a weekend or overnight closure. Janiform uses session information when it interprets timestamps, gaps, and market activity.

## Exchanges

An exchange is a venue or market calendar source, such as a cash equity exchange, a futures venue, an FX spot convention, or a crypto market.

Different exchanges have different calendars and trading rhythms. Some markets close daily. Some trade nearly continuously during the week. Some trade continuously every day. Janiform uses exchange and session information to apply the right timing assumptions.

## Market profiles

A market profile is a preset that describes a market setup. It can point to a session, known news defaults, timestamp conventions, an optional tick-size fallback, and input-column hints.

Market profiles reduce setup work. For example, a bundled EUR/USD spot profile can carry the right FX session and known-news suggestions. You can still override details when the data or workflow requires it.

In the first model walkthrough, Janiform infers the bundled `eurusd_spot` profile from the synthetic EUR/USD file name.

## Fitting

Fitting is the process of learning model parameters from data. The fitted model is not a copy of the data. It is a compact description of behavior learned from the data.

For Janiform, fitting can learn return behavior, activity behavior, buy and sell pressure dynamics, seasonal structure, and other workflow-specific components. The exact fitted pieces depend on the workflow and available inputs.

## Simulation

Simulation uses a fitted model to generate new price paths.

A simulation can be useful for testing strategies, risk controls, monitoring systems, execution logic, data pipelines, and pricing experiments. It gives you many plausible paths rather than one historical path.

Janiform is designed so the same fitted model can simulate fine tick-level behavior and coarser paths without switching to a separate model. This is one of the core strengths of Janiform.

## Derivatives

A derivative is a financial contract whose value depends on an underlying market. An option is a common derivative. A call option benefits when the underlying price rises above a strike. A put option benefits when the underlying price falls below a strike.

Derivatives are not just sensitive to the average future price, but to the distribution of future prices. Volatility, jumps, gaps, activity clustering, and directional pressure can all matter.

## Real-world and risk-neutral views

The real-world view describes how markets appear to behave in observed data. It is useful for simulation, scenario generation, system testing, and understanding plausible market paths.

The risk-neutral view is used for derivative pricing. It is the pricing view where expected discounted payoffs can be turned into prices under the pricing assumptions.

The distinction matters because a realistic historical simulation is not automatically a derivative price. Pricing needs to respect the pricing measure, while still caring deeply about the shape of future paths.

Janiform helps by carrying realistic fitted dynamics into pricing workflows. You can study derivative values with dynamics that include clustered activity, jumps, gaps, and buy/sell pressure rather than relying on a flat volatility assumption.

## Why realistic dynamics matter for options

Options care about the whole path distribution. A model that gets the average volatility right can still miss important behavior.

Clustered activity can change how likely extreme paths are. Jumps and gaps can change tail risk. Buy and sell pressure can affect asymmetry. Market sessions can change how risk accumulates across open and closed periods.

These effects can change option values, implied volatility patterns, and the comparison between model-implied prices and observed market prices.

## How the pieces fit together

The basic Janiform workflow is:

1. Start with market data and market context.
2. Fit a Janiform model.
3. Inspect the fitted behavior and simulated paths.
4. Check whether the fitted behavior is realistic enough for the job.
5. The model can then be used for simulation or derivative pricing.

The important idea is that these are not separate assumptions glued together after the fact. The fitted model provides one shared structure for activity, returns, simulation, and pricing workflows.

## Next steps

- [First model](first-model.md)
- [Fit models](../workflows/fit-models.md)
- [Simulate paths](../workflows/simulate-paths.md)
- [Price derivatives](../workflows/price-derivatives.md)
