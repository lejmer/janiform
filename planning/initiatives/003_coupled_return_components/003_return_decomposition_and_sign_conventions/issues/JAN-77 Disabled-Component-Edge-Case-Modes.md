## Disabled Component Edge Case Modes

### Description
Implement explicit edge-case modes where one decomposition component is disabled to isolate correctness and simplify debugging.

### Acceptance Criteria
- System supports buy-only, sell-only, and gap-only decomposition test modes
- Disabled components contribute exactly zero in both simulation and CF assembly paths
- Edge-mode behavior is documented and stable across engines
- Edge modes are available in test and debug workflows

### Unit tests
- Buy-only mode with zero sell and gap contributions, sell-only mode with correct negative sign behavior, and gap-only mode isolating `G` contribution

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Keep edge modes behind explicit configuration to avoid accidental production use
- Ensure mode behavior is deterministic under fixed seeds and inputs
