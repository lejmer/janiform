## Stylized Fact Metric Catalog And Schema

### Description
Define the canonical metric catalog and schema for stylized-fact diagnostics across activity clustering, tails, skew, volatility persistence, open-window direct-jump behavior, and closed-window gap behavior.

### Acceptance Criteria
- Metric catalog includes unique IDs, definitions, units, and horizon applicability
- Metric catalog classifies metrics as `primary_robust` or `secondary_moment`
- Primary robust catalog includes quantile-shape, tail-risk, and distribution-distance families
- Secondary moment catalog includes L-moment and classical central-moment variants with estimator metadata
- Naming conventions are consistent across all diagnostics outputs
- Catalog schema includes uncertainty method, minimum sample-sufficiency requirements, and `insufficient_data` status semantics
- Catalog defines block-bootstrap uncertainty as the default for dependent return-series diagnostics, with override fields for approved alternatives
- Output schema and metadata are machine-readable and documented

### Unit tests
- Required-field coverage per metric category
- Schema validation on valid and invalid metric definitions
- Validation of metric-class-specific required fields and uncertainty metadata

### Notes/Constraints
- Keep catalog decoupled from specific plotting or report layers
- Avoid duplicate metric names with conflicting definitions
- Pre-v0.1 policy applies: breaking schema changes are allowed when they improve model correctness and consistency.
