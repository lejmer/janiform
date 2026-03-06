## Artifact Manifest Schema And Versioning Policy

### Description
Define the top-level artifact manifest schema and versioning policy for persisted calibration runs, including explicit pre-v0.1 schema-evolution rules.

### Acceptance Criteria
- A canonical manifest schema exists with run ID, artifact references, and schema version for persisted runs.
- Versioning policy defines pre-v0.1 schema-evolution behavior where breaking changes are allowed with explicit version bumps and release notes.
- Manifest validation rejects missing required fields and invalid versions.
- Policy documentation includes upgrade and deprecation guidance.
- Policy documentation explicitly states persisted artifacts are optional, with defaults set to ON for CLI workflows and OFF for imported package usage unless explicitly enabled.
- Documentation scopes manifest requirements to `execution_mode=disk` flows.

### Unit tests
- Required-field enforcement.
- Manifest schema validation on valid and invalid payloads and schema-evolution checks and expected failure cases.

### Notes/Constraints
- Keep manifest stable and minimal to reduce migration overhead.
- Avoid embedding large payloads directly in the manifest.
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
