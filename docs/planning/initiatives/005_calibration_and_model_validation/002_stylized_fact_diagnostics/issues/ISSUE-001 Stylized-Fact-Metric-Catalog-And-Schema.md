## ISSUE-001: Stylized Fact Metric Catalog And Schema

### Description
Define the canonical metric catalog and schema for stylized-fact diagnostics across activity clustering, tails, skew, volatility persistence, and gap behavior.

### Acceptance Criteria
- Metric catalog includes unique IDs, definitions, units, and horizon applicability
- Naming conventions are consistent across all diagnostics outputs
- Output schema and metadata are machine-readable and documented

### Unit tests
- Required-field coverage per metric category
- Schema validation on valid and invalid metric definitions

### Notes/Constraints
- Keep catalog decoupled from specific plotting or report layers
- Avoid duplicate metric names with conflicting definitions
