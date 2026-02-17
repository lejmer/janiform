## ISSUE-008: Experimental Endpoint Isolation And Feature Flags

### Description
Establish mechanism to isolate experimental Python endpoints from stable v0.1 contracts while still allowing controlled iteration.

### Acceptance Criteria
- Experimental APIs are namespaced or flagged separately from stable APIs
- Stable contract checks exclude experimental interfaces by default
- Documentation explains how experimental features are enabled and supported
- Experimental endpoint lifecycle policy is documented

### Unit tests
- Tests ensuring experimental endpoints are not imported from stable public namespaces, feature-flag or namespace gating behavior, and stable API behavior is unchanged when experimental features are disabled

### Notes/Constraints
- Keep experimental mechanisms lightweight and explicit
- Do not introduce ambiguity about stability guarantees
