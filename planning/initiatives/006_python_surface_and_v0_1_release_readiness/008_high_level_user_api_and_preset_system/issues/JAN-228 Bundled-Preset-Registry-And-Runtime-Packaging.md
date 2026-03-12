## Bundled Preset Registry And Runtime Packaging

### Description
Implement the runtime registry layer for bundled and user-defined presets so Janiform can resolve maintained session, known-news, and market-profile defaults by name, inspect them, and export them without treating presets as informal examples.

### Implementation details
- Define a public registry abstraction that can:
  - resolve built-in presets
  - register user-defined presets in memory
  - load user-defined presets from JSON
  - export resolved preset artifacts for inspection
- Keep built-in and user registries conceptually separate even if the resolver can compose them into one lookup path.
- Wire the runtime registry to deterministic generated artifacts instead of hand-maintained example files, following the architecture described in `docs/design/programmatic_preset_generation_plan.md`.
- Define the minimal bundled v1 preset set called for by the design note:
  - session presets: `XNYS`, `XNAS`, `fx_spot_24_5`, `crypto_24_7`
  - known-news presets: `none`, `us_macro_core`, `eu_us_macro_fx_core`
  - market profiles: `eurusd_spot`, `us_equity_cash`, `btcusd_spot`
- Define how bundled preset metadata exposes provenance, version labels, and content identity without requiring users to inspect generator internals.
- Ensure runtime packaging works the same way for Python importable APIs and CLI preset-discovery commands.

### Acceptance Criteria
- Planning defines one runtime registry abstraction for bundled and user-defined presets across the high-level API and CLI.
- Bundled presets are treated as maintained runtime artifacts with provenance and inspection support, not as example-only configuration.
- The minimal v1 preset set is explicitly listed and assigned to runtime packaging and validation work.
- User-defined JSON loading and export behavior is defined with deterministic output rules.

### Unit tests
- Registry lookup and duplicate-name handling tests for bundled and user-defined entries.
- Deterministic export and serialization tests for resolved presets.
- Snapshot or regression tests for the initial bundled preset set and runtime index manifests.
- Negative tests for malformed preset JSON and dangling market-profile references.

### Notes/Constraints
- Coordinate generator mechanics, source manifests, and maintenance workflow with the separate preset-generation design note rather than duplicating that whole plan here.
- Keep runtime lookup cheap and inspectable. Do not add hidden network fetches or live external refresh behavior.
