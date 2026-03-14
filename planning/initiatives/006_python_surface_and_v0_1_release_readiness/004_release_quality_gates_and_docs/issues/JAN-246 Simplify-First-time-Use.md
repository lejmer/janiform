## Simplify First-time Use

### Description

Make the first-time path task-shaped, not contract-shaped. The user should mostly answer "where is my data?" and "what market is this?", and Janiform should infer or default the rest.

**Python Library**

- Make `JaniformEstimator()` usable with no required config for the common tick-data path.
- Default `workflow_scope` to `FULL_WITHOUT_OPTION_SURFACE`.
- Default `normalization_policy` internally instead of forcing users to construct it.
- Infer `tick_data_format` from file extension if not explicitly set.
  Infer `tick_price_input_policy` from available columns:
    - `price`, `midprice` etc -> assume explicit price
    - If a `trade_sign` or `trade_direction` column also exists -> assume trade-price mode
    - `bid` + `ask` -> bid/ask midpoint
    - `close` -> close-price mode
- Auto-resolve common timestamp and price column names from aliases.
- Let market_profile drive more defaults:
    - timezone
    - session preset
    - known-news recommendation or application
    - column alias hints
    - default calibration hints
- A flag for silencing should be available: when silenced, do not output anything except errors and warnings.

Let `run_id` be optional and auto-generate one from file stem plus timestamp.

Keep the expert knobs, but move them off the minimum path. The ideal import story is: users import only `JaniformEstimator`, and only touch enums/contracts when overriding defaults.

**CLI**

- Make the tick-data path positional for the simple path.
- Infer file format from the path unless explicitly overridden.
- Default `--workflow-scope` to `full_without_option_surface`.
- Replace required column flags with auto-detection by default, and add override flags only when needed.
- Make `--market` the main beginner flag. For many users, `janiform fit eurusd_ticks.csv --market eurusd_spot` should be enough.
- Auto-generate `--run-id` if omitted.
- Keep advanced flags available, but separate them mentally or structurally from the first-time path.
- Add a lightweight "explain what I inferred" mode:
    - show detected format, timestamp column, price policy, timezone, session source, known-news choice
    - this builds trust without forcing explicit config
- A flag for silencing should be available: turns on the silencing mode the Python library makes available above.
- When inference fails, produce prescriptive errors:
    - "Found timestamp candidates: ..."
    - "Found price candidates: ..."
    - "Please set --tick-price-column or --tick-price-input-policy."
- Consider a dedicated inspection command like `janiform fit inspect` or `janiform fit validate` or something, that previews schema detection before a full fit.

**Product-level ideas**

- Use a two-layer model explicitly:
    - beginner path: simple defaults and inference
    - expert path: full explicit control
- Bias toward "sensible auto, explicit override" everywhere.
- Make inference visible in summaries and reports so users learn the model gradually.
- Prefer market presets over raw low-level flags. We already have good preset/session machinery, so the UX win is mostly surfacing it better.
- Add a few curated "known-good" examples centered on simple workflows:
    - FX spot ticks
    - US equities regular session
    - crypto 24/7

The key bar for first-time UX is: if the market preset is known and the CSV headers are conventional, the user should not need enums, column-mapping objects, or normalization-policy objects at all.
