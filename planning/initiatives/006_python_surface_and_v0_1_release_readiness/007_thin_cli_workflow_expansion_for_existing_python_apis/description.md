## Project 007: Thin CLI Workflow Expansion For Existing Python APIs

Deliver a focused second wave of CLI workflows by exposing capabilities that already exist in the public Python surface, so Janiform closes obvious usability gaps without requiring new Rust core features or major PyO3 boundary work.

### Context and requirements
- Add CLI commands only for capabilities that already have stable public Python entry points and existing contract validation.
- Prefer thin command implementations that reuse current `python/janiform/cli` parser, output-mode, and error-classification patterns.
- Keep command design task-oriented and file-driven so the CLI remains useful for scripting and automation.
- Avoid introducing parallel contract shapes when an existing Python contract or report shape already exists.
- Update CLI help, command reference docs, and regression coverage in the same change as each command.

### Definition of Done
- The CLI exposes thin commands for the selected existing Python workflows without requiring new Rust core features.
- Each command supports structured output suitable for automation and follows existing CLI error and exit-code conventions.
- Command help text, docs, and examples describe the same request and output shape.
- CLI regression coverage exists for argument parsing, successful execution, and representative failure cases.
- New commands fit the existing namespace layout and do not make the top-level CLI harder to navigate.

### Agent implementation notes
- Prefer adding one command at a time with tight workflow boundaries rather than introducing a broad generic "run native function" command.
- Reuse public Python module entry points directly where possible instead of reaching into private helpers.
- Keep namespace choices aligned with existing `simulation`, `calibration`, `pricing`, and `dev` command patterns.

### Initiative alignment
- Extends Initiative 006 with low-risk CLI surface growth that improves user-facing completeness before v0.1.
- Strengthens the success signal that Python API and CLI expose the most important production and validation workflows backed by the Rust core.
- Reduces the gap between already-public Python capabilities and scriptable CLI access.
