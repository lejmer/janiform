# Presets And Data

Combine presets with explicit file inputs for Janiform workflows. Read [../getting-started/concepts.md](../getting-started/concepts.md) to learn what terms like market profile, session, and known news mean. Use the reference docs for the full flag list.

## Preset Categories

The CLI exposes three preset categories:

- `session`
- `known-news`
- `market-profile`

Use `janiform presets list` to see what is bundled, `janiform presets show` to inspect one preset, and `janiform presets export` when you want a resolved preset plus nested dependencies written out. Add `--expanded` to `janiform presets show known-news NAME` or `janiform presets show session NAME` when you want category-specific detail tables in the human terminal view.

User presets are registered explicitly for each command. Pass a single preset file with `--user-preset PATH`, or pass a manifest with `--preset-registry PATH`:

```json
{
  "schema_version": "preset.user_registry.v1",
  "preset_artifact_paths": [
    "sessions/my_session.json",
    "market_profiles/my_profile.json"
  ]
}
```

Manifest paths are resolved relative to the manifest file unless they are absolute.

## Calibration Inputs

### Tick data

- Required for fitting
- Supported formats: `csv`, `parquet`
- Common fields: timestamp, price or bid/ask columns, and trade-sign data for full-model Hawkes calibration

### Known news

- Optional for fitting
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
