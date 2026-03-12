## Preset Reference And Configuration Contracts

### Description
Define the public preset reference and configuration boundary models used by the high-level API so users can choose bundled presets, provide explicit configuration, or attach small override patches without falling back to loose ad hoc keyword arguments.

### Implementation details
- Define explicit boundary value objects for the preset entry model, including the conceptual roles described in the design note such as `SessionPresetReference`, `KnownNewsPresetReference`, and `MarketProfileReference`.
- Support the four intended usage modes in one contract family:
  - automatic session-inference default usage
  - name-only preset usage
  - preset plus override patch usage
  - fully explicit configuration usage
- Define category-specific fields and validation rules for session, known-news, and market-profile references instead of one untyped catch-all preset blob.
- Specify how references can resolve from built-in names, user-registered names, explicit dictionaries, and JSON documents.
- Keep high-level workflow configuration bounded. Preset references should describe user intent and policy, not leak internal staged execution payloads into the public API.
- Define deterministic serialization and inspection behavior so preset references and explicit overrides can be logged, diffed, and persisted in provenance records.

### Acceptance Criteria
- Public planning defines typed preset-reference and explicit-configuration contracts for the high-level Python and CLI surface.
- The contract family covers named presets, explicit configuration, override patches, and inferred session usage without relying on undocumented keyword conventions.
- Validation rules distinguish invalid category usage, ambiguous overrides, and unsupported combinations with actionable guidance.
- Serialization and inspection behavior is deterministic enough for provenance and regression checks.

### Unit tests
- Construction and validation tests for valid and invalid preset references by category.
- Serialization and round-trip tests for named presets, inline explicit configuration, and override-patch cases.
- Negative tests for ambiguous or contradictory preset-reference combinations.

### Notes/Constraints
- Pre-v0.1 policy applies. Replace weaker public contract shapes directly instead of carrying dual preset-entry patterns.
- Keep the public boundary small and user-oriented. Do not expose raw internal execution-contract fragments as the preset API.
