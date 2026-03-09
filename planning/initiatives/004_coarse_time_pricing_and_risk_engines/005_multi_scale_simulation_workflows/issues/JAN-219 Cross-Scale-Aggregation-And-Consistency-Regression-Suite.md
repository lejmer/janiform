## Cross Scale Aggregation And Consistency Regression Suite

### Description
Add regression coverage that compares coarse-interval simulation outputs against aggregated fine-scale seeded-tick behavior where the model is expected to agree up to the intended approximation envelope.

The design note explicitly commits Janiform to projective or semigroup-style scale consistency as a practical modeling goal. Once the coarse simulation workflow exists, the repository should test that it remains meaningfully aligned with fine-scale generation instead of drifting independently.

### Acceptance Criteria
- Regression fixtures and tests compare coarse simulation outputs against aggregated seeded-tick outputs for representative scenarios.
- Coverage includes at least hourly reference cases and one compatible non-hourly interval case.
- Reported comparisons make clear which quantities are expected to match exactly, approximately, or only distributionally.
- Regression failures provide useful diagnostics for whether drift arises from sufficient-statistics sampling, direct-jump generation, or state carryover.

### Unit tests
- Deterministic scenario tests for interval count, boundary alignment, and component-routing semantics.
- Statistical or tolerance-based regression checks for aggregated fine versus direct coarse behavior.
- Failure-diagnostic tests ensuring regression reports remain machine-readable and stable.

### Notes/Constraints
- Do not overstate exact equality where the model only promises approximation or distributional alignment.
- Reuse existing scale-consistency and validation infrastructure where appropriate instead of inventing a parallel regression harness family.
