## ISSUE-001: Artifact Manifest Schema And Versioning Policy

### Description
Define the top-level artifact manifest schema and versioning policy for calibration runs, including compatibility rules for v0.x.

### Acceptance Criteria
- A canonical manifest schema exists with run ID, artifact references, and schema version
- Versioning policy defines compatibility guarantees for v0.x artifacts
- Manifest validation rejects missing required fields and invalid versions
- Policy documentation includes upgrade and deprecation guidance
- Policy documentation explicitly states persisted artifacts are optional, with defaults set to ON for CLI workflows and OFF for imported package usage unless explicitly enabled

### Unit tests
- Required-field enforcement
- Manifest schema validation on valid and invalid payloads and version compatibility checks and expected failure cases

### Notes/Constraints
- Keep manifest stable and minimal to reduce migration overhead
- Avoid embedding large payloads directly in the manifest
