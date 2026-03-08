## Double Exponential Base Shock Sampler

### Description
Implement Kou-style double-exponential base shock sampler for `epsilon` marks, including constant mixture probability `p` and side-specific decay parameters.

### Acceptance Criteria
- Sampler supports parameters `p`, `eta_plus`, and `eta_minus` with domain validation
- Sampling behavior is deterministic under fixed seed and configuration
- Constant `p` behavior is explicit and documented for v0.1 scope
- Output is reusable for closed-market gap mark construction

### Unit tests
- Distribution-shape sanity tests for positive and negative tails
- Parameter validation tests for valid and invalid `p`, `eta_plus`, `eta_minus` values
- Seed reproducibility tests for repeated base-shock sampling

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Keep this sampler independent from severity-mark generation
- Do not introduce dynamic `p` behavior in v0.1
