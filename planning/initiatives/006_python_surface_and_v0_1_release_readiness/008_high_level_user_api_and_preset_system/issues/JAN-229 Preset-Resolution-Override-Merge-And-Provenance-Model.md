## Preset Resolution Override Merge And Provenance Model

### Description
Define the canonical resolution pipeline that combines inferred session behavior, bundled preset baselines, user overrides, and fully explicit replacement while preserving exact provenance for the fitted model and related diagnostics.

### Implementation details
- Define one resolved-configuration model that all high-level workflows consume after preset lookup and session inference complete.
- Encode the recommended precedence model from the design note:
  1. inferred session behavior or bundled preset baseline
  2. user override patch
  3. full explicit replacement when supplied
- Apply that precedence model separately and explicitly to session settings, known-news settings, and market-profile settings.
- Define how automatic session inference feeds the same resolved session model with provenance and confidence diagnostics instead of a hidden side channel.
- Define explicit known-news policy semantics so the default high-level behavior is `known_news="none"` and recommended profile defaults remain inspectable rather than silently active.
- Require fitted models and fit reports to record:
  - which market profile or preset was selected, if any
  - which session preset was selected, if any
  - whether session behavior was inferred from data
  - which known-news preset was selected, if any
  - whether each component was built-in, overridden, inferred, or fully custom
- Define user-facing diagnostics for ambiguous session inference, conflicting overrides, and profile-to-component resolution outcomes.

### Acceptance Criteria
- Planning defines one canonical resolved-configuration and provenance model for high-level workflows.
- Precedence and merge behavior is deterministic and category-aware.
- Session inference plugs into the same resolution model rather than creating a parallel calendar stack.
- Fitted-model and fit-report provenance requirements are explicit enough for reproducibility and debugging.
- Known-news default and recommendation semantics are documented and non-ambiguous.

### Unit tests
- Resolution tests covering named presets, inferred-session paths, override merges, and full explicit replacements.
- Provenance tests asserting resolved outputs record built-in, inferred, overridden, and custom component sources correctly.
- Ambiguity tests for conflicting overrides and low-confidence session inference outcomes.

### Notes/Constraints
- Coordinate automatic session inference behavior with Initiative 009 planning and the existing calendar machinery under Initiative 001.
- Do not silently apply known-news behavior-changing defaults just because a market profile suggests them.
