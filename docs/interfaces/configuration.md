# Configuration

The CLI supports one global configuration file through `--config`. That file is a TOML document for CLI defaults.

## What the file does

The configuration file sets defaults for global CLI output behavior:

- color
- quiet mode
- silent mode
- verbose mode
- JSON mode
- event streaming
- trace events

Command-specific inputs such as tick data paths, model paths, strikes, expiries, and calibration settings stay on the command line.

## File shape

The current configuration file has one top-level table:

```toml
[output]
```

Unknown top-level fields and unknown fields inside `[output]` are rejected.

## Example

```toml
[output]
color = "auto"
quiet = false
silent = false
verbose = 1
json = false
events = "jsonl"
trace_events = false
```

Every field in `[output]` is optional. If a field is omitted, the CLI uses its built-in default or a more specific command-line flag.

## Loading the file

Pass the file path with `--config`:

```bash
janiform --config config/janiform.toml fit data/Synthetic_EURUSD_Data_1M_ticks.parquet
```

The `--config` path is resolved relative to the current working directory unless it is already absolute.

If `--config` is omitted, the CLI uses its built-in defaults.

## Precedence

Command-line flags take precedence over the configuration file.

For output mode:

1. `--json`, `--silent`, and `--quiet` on the command line win when present
2. otherwise the CLI reads `output.json`, then `output.silent`, then `output.quiet`
3. if none are set, the CLI uses the default terminal output mode

For other settings:

- `--color` overrides `output.color`
- `-v` or `-vv` override `output.verbose`
- `--events` overrides `output.events`
- `--trace-events` is enabled if either the flag is passed or `output.trace_events = true`

## TOML table: `[output]`

### TOML field: `color`

Controls ANSI color in terminal output.

Accepted values:

- `auto`
- `always`
- `never`

```toml
[output]
color = "auto"
```

Meaning:

- `auto` enables color when the terminal supports it
- `always` forces color in terminal output
- `never` disables color

JSON output and JSONL event streams do not include ANSI color.

### TOML field: `quiet`

Enables quiet terminal output.

```toml
[output]
quiet = true
```

Quiet mode keeps the output concise and focuses on final status and output paths.

### TOML field: `silent`

Suppresses normal output while still allowing warnings and errors.

```toml
[output]
silent = true
```

Silent mode fits jobs where the exit code and failure diagnostics matter more than normal command output.

### TOML field: `verbose`

Sets terminal diagnostic verbosity.

```toml
[output]
verbose = 1
```

Meaning:

- `0` is the default
- `1` matches `-v`
- `2` matches `-vv`

Higher values are accepted as integers, but the documented CLI levels are `-v` and `-vv`.

### TOML field: `json`

Enables final JSON output on stdout.

```toml
[output]
json = true
```

With JSON mode enabled:

- stdout contains one final JSON object
- terminal progress and terminal summaries are suppressed on stdout
- machine event streaming can still be enabled separately with `events = "jsonl"`

### TOML field: `events`

Enables machine-readable event streaming to stderr.

Accepted values:

- `jsonl`

```toml
[output]
events = "jsonl"
```

This writes one JSON event object per line to stderr.

### TOML field: `trace_events`

Adds trace-level records to the configured event stream.

```toml
[output]
trace_events = true
```

This setting requires `events = "jsonl"`.

## Validation rules

The CLI rejects configuration files with conflicting output fields.

The current conflict rules are:

- `output.json` conflicts with `output.silent`
- `output.json` conflicts with `output.quiet`
- `output.silent` conflicts with `output.quiet`
- `output.verbose` conflicts with `output.silent`
- `output.verbose` conflicts with `output.quiet`
- `output.trace_events` requires `output.events`

For example, this is invalid:

```toml
[output]
json = true
quiet = true
```

And this is also invalid:

```toml
[output]
trace_events = true
```

because no event stream is configured.

## Common patterns

### Default JSON mode

```toml
[output]
json = true
```

### Default event streaming

```toml
[output]
events = "jsonl"
```

### Quiet terminal output with event streaming

```toml
[output]
quiet = true
events = "jsonl"
```

### Color disabled

```toml
[output]
color = "never"
```

## What the file does not configure

The configuration file does not carry workflow inputs such as:

- tick data paths
- model paths
- pricing terms
- fit-specific market-data inputs
- command-specific output paths

Those stay explicit on each command.

## See also

- [Command line interface](cli.md)
- [External integration](external-integration.md)
- [CLI reference](../reference/cli.md)
