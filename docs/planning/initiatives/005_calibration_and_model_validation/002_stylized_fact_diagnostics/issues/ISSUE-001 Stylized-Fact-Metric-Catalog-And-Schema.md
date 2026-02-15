## ISSUE-001: Stylized Fact Metric Catalog And Schema

### Description
Define the canonical metric catalog and schema for stylized-fact diagnostics across activity clustering, tails, skew, volatility persistence, and gap behavior.

### Acceptance Criteria
- Metric catalog includes unique IDs, definitions, units, and horizon applicability.
- Schema is machine-readable and versioned for artifact compatibility.
- Naming conventions are consistent across all diagnostics outputs.
- Schema validation rejects incomplete or malformed metric records.

### Unit tests
- Tests for schema validation on valid and invalid metric definitions.
- Tests for required-field coverage per metric category.

### Notes/Constraints
- Keep catalog decoupled from specific plotting or report layers.
- Avoid duplicate metric names with conflicting definitions.
