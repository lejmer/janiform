## Initiative 001: Market Calendar And Seasonality Foundation

Build the deterministic calendar and seasonality backbone that feeds all Hawkes and coarse-time components with identifiable, reusable time effects.

### Desired outcome
- Janiform has a production-ready calendar-time layer that models market open and closed windows and deterministic activity seasonality.
- The system can produce normalized seasonality factors `s(t)` and closed-window news intensity baselines `nu_news(t)` at hourly resolution for any timestamp in supported exchanges.
- The core model uses this layer as a stable upstream input so excitation dynamics and baseline rates are identifiable and not confounded by calendar effects.

### Success signals
- A canonical market calendar component exists and is used consistently by both Rust core and Python entrypoints.
- Seasonality estimation supports intraday, day-of-week, and day-of-year effects and outputs a normalized signal with mean level anchored (for example, average `s(t) = 1` over the fit window).
- Hourly piecewise-constant values (`s_h`, `nu_h`) are generated and consumed by simulation and pricing paths.
- Tests cover normal sessions, weekends, holidays, and market closures with deterministic expected outputs.
