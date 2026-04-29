# Presets And Data

Combine presets with explicit file inputs for high-level workflows. Read [../getting-started/concepts.md](../getting-started/concepts.md) to learn what terms like market profile, session, and known news mean. Use the reference docs for the full flag list.

## Preset Categories

The CLI exposes three preset categories:

- `session`
- `known-news`
- `market-profile`

Use `janiform presets list` to see what is bundled, `janiform presets show` to inspect one artifact, and `janiform presets export` when you want a resolved preset plus nested dependencies written out.

## Calibration Inputs

### Tick data

- Required for high-level fitting
- Supported formats: `csv`, `parquet`
- Common fields: timestamp, price or bid/ask columns, and trade-sign data for full-model Hawkes calibration

### Known news

- Optional for high-level fitting
- Supported formats: `csv`, `parquet`, `json`
- Used directly in explicit market-data planning and staged workflows

### Option surface

- Optional unless you want to fit the model with option data
- Supported formats: `csv`, `parquet`
- Supply quote timestamp, expiry, strike, option type, and price quotes with bid and ask data

### Calendar registry

- JSON input used by seeded simulation, staged calibration, seasonality workflows, hourly-factor materialization, and deterministic regression commands

## Paths

CLI JSON and data-path flags resolve relative paths from the project root.
