## ISSUE-008: Experimental Endpoint Isolation And Feature Flags

### Description
Establish mechanism to isolate preview Python endpoints from the primary public API while still allowing controlled iteration during pre-v0.1 development.

### Acceptance Criteria
- Preview APIs are namespaced or flagged separately from primary APIs
- Primary contract checks exclude preview interfaces by default
- Documentation explains how experimental features are enabled and supported
- Experimental endpoint lifecycle policy is documented

### Unit tests
- Tests ensuring preview endpoints are not imported from primary public namespaces, feature-flag or namespace gating behavior, and primary API behavior is unchanged when preview features are disabled

### Notes/Constraints
- Keep experimental mechanisms lightweight and explicit
- Do not introduce ambiguity about which namespace is primary versus preview
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
