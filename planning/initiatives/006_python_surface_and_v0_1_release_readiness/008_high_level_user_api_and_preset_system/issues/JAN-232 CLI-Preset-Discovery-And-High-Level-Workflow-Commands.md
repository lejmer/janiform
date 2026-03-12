## CLI Preset Discovery And High-Level Workflow Commands

### Description
Reshape the CLI around the same high-level workflow and preset mental model as the new Python API so users can inspect bundled defaults, fit with named profiles, and simulate later without dropping into internal contract files as the normal path.

### Implementation details
- Add CLI preset-discovery capabilities aligned with the design note:
  - `janiform presets list`
  - `janiform presets show ...`
  - `janiform presets export ...`
- Define the task-oriented high-level workflow command family for fitting and simulation, including the conceptual paths:
  - `janiform fit ...`
  - `janiform simulate ...`
  - `janiform fit-and-simulate ...`
- Map CLI options to the preset entry model and resolved-configuration logic, including:
  - omitted session input with automatic inference when confidence is sufficient
  - `--market ...`
  - `--session-preset ...`
  - `--known-news ...`
  - `--known-news none`
  - explicit session and known-news JSON override paths
- Keep low-level staged-execution and debugging commands available as advanced surfaces, but stop making them the primary onboarding story. They should be one level below where they are now.
- Align command help, output schemas, and error behavior with the preset registry and provenance model so users can see what was inferred, selected, and overridden.

### Acceptance Criteria
- Planning defines CLI preset discovery and task-oriented high-level workflow commands that match the same conceptual model as the high-level Python API.
- Named presets, explicit overrides, and automatic session inference are exposed through clear command flags and structured output.
- Advanced staged-contract workflows remain available but are explicitly secondary to the high-level commands.
- CLI docs and output planning expose resolved preset and provenance information rather than hiding it in internal-only diagnostics.

### Unit tests
- Parser and argument-mapping tests for preset-discovery commands and the high-level workflow commands.
- Structured-output tests for resolved preset metadata and provenance sections.
- Failure-path tests for ambiguous session inference, invalid preset names, and contradictory override combinations.
- Regression tests confirming advanced staged-execution commands remain intentionally available but are not required for the default path.

### Notes/Constraints
- If the current CLI namespace layout conflicts with this simpler task-oriented public UX, prefer replacing or demoting the older surface instead of keeping two equal-status command families.
- Keep command naming aligned with the final high-level Python API terminology to reduce cognitive load.
