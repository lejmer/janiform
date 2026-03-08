## Stylized Fact Report Generator CLI Command

### Description
Add a CLI command for generating stylized-fact horizon-comparison reports using the already-public Python workflow entry point.

The command should make it easy to supply a metric catalog, comparison request, and optional artifact-export settings from files, then emit the validated report bundle. This is a workflow and reporting command, not a low-level binding exposure.

### Acceptance Criteria
- The CLI exposes a stylized-fact report-generation command in a namespace consistent with the current CLI architecture.
- The command accepts file-driven metric-catalog and comparison-request inputs and supports execution-mode selection that matches the Python workflow.
- When artifact export is requested, the CLI passes the export configuration through the existing Python validation path instead of inventing a separate CLI-only contract.
- The command emits the validated report bundle in JSON mode and provides useful human-readable summary output.
- CLI docs and examples cover both in-memory report generation and disk-backed artifact export.

### Unit tests
- Parser tests for the command path, execution-mode handling, and artifact-export arguments.
- CLI tests validating successful report generation from fixture inputs.
- Failure-path tests for invalid execution mode, invalid metric catalog payload, and invalid artifact-export configuration.

### Notes/Constraints
- Reuse the public `janiform.stylized_facts` workflow rather than exposing private native calls directly.
- Keep any human-readable rendering as a summary view over the canonical report bundle, not a replacement for it.
