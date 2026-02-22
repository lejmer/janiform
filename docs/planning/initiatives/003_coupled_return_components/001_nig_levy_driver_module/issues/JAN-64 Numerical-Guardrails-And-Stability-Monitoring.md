## Numerical Guardrails And Stability Monitoring

### Description
Add numerical guardrails for near-boundary NIG parameter regimes and transform arguments, with diagnostics that help identify unstable evaluations.

### Acceptance Criteria
- Guardrails detect near-domain-violation inputs and report structured warnings or errors
- Evaluation path includes stable handling policies for sensitive numeric regions
- Output schema and metadata are machine-readable and documented

### Unit tests
- Stable behavior under representative extreme argument ranges
- Diagnostic payload schema and required fields
- Guardrail triggers near `alpha` and `beta` boundary conditions

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Keep guardrail behavior deterministic
- Do not silently clip invalid inputs without reporting
