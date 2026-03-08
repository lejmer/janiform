## Project 005: Market Data Calibration Workflows

Deliver a market-data-first calibration surface so users can calibrate from familiar finance inputs like `csv` or `parquet` files, session definitions, and known-news calendars instead of constructing staged execution payloads by hand.

### Context and requirements
- Add a new user-facing calibration workflow above the existing staged calibration engine.
- Accept raw or lightly prepared market data inputs such as tick data files, optional option-surface files, session definitions, known-news calendars, and explicit column mappings where needed.
- Keep the existing staged calibration execution contract as an advanced or internal boundary for debugging, replay, and low-level control.
- Derive normalized staged calibration inputs inside Janiform with explicit provenance, validation, and deterministic preprocessing rules.
- Make the default Python and CLI calibration UX match what market professionals expect when they hear "calibrate to market data".

### Definition of Done
- Planning defines a clear user-facing calibration request and CLI shape for file-driven workflows.
- Issue set covers file ingestion, schema and column mapping, preprocessing and staged-input derivation, and user-facing docs plus workflow validation.
- The new workflow preserves a clean boundary between user-facing ingestion and the existing staged calibration execution engine.
- Release and documentation planning are aligned so market-data-first calibration is treated as a primary v0.1 workflow.

### Agent implementation notes
- Prefer progressive disclosure: simple workflow entrypoints first, advanced staged contracts second.
- Keep provenance explicit so artifacts can always explain which files, mappings, and normalization policies were used.
- Avoid duplicating calibration logic. User-facing workflows should translate into the canonical staged calibration path rather than creating a second execution engine.
- Treat schema inference as helpful but never magical. Ambiguous file layouts should fail with actionable guidance and explicit override hooks.

### Initiative alignment
- Extends Initiative 006 from engineering-facing Python and CLI contracts to workflows that match market-user expectations.
- Reduces the risk that v0.1 is technically polished but operationally unnatural for the people most likely to calibrate the model.
