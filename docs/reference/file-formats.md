# File formats

Janiform uses a small set of file formats for configuration, saved artifacts, event streams, market-data inputs, and exported results.

## Summary

| Format     | Example                                                                 | Description                                                      |
|------------|-------------------------------------------------------------------------|------------------------------------------------------------------|
| TOML       | `janiform.toml`                                                         | Structured key-value configuration documents                     |
| `.jf`      | `model.jf`                                                              | Janiform-native saved artifacts                                  |
| JSON       | `report.json`, `pricing.json`, `simulation_report.json`, `run.json`     | Structured data exchange and artifact documents                  |
| JSON Lines | `events.jsonl`                                                          | Record streams with one structured object per line               |
| CSV        | `ticks.csv`, `options.csv`, `simulation.csv`, `resolved-known-news.csv` | Row-oriented tabular data                                        |
| Parquet    | `ticks.parquet`, `options.parquet`, `simulation.parquet`                | Columnar tabular data                                            |
| HTML       | `report.html`                                                           | Rendered document output                                         |

## TOML

TOML appears in configuration-style documents with named fields and tables.

The main documented example is the configuration file loaded through `--config`.

## Janiform-native

`.jf` is a Janiform artifact extension for saved model files such as `model.jf`.

The extension identifies the artifact as a Janiform model file.

## JSON

JSON appears across structured data documents and workflow artifacts.

## JSON Lines

JSON Lines, also written as JSONL, appears in streamed record logs where each line is one complete JSON object.

The main example is `events.jsonl`.

## CSV

CSV appears in row-oriented tabular inputs and outputs.

## Parquet

Parquet appears in columnar tabular inputs and outputs.

## HTML

HTML appears in rendered document output.

The main example is a rendered fit report such as `report.html`.

## See also

- [Configuration](../interfaces/configuration.md)
- [External integration](../interfaces/external-integration.md)
- [Inspect and report](../workflows/inspect-and-report.md)
