## ISSUE-003: Reference Generation And Source Of Truth Policy

### Description
Define how Janiform will maintain CLI, Python API, schema, and configuration reference inside the Docusaurus site without letting the docs drift from code. This issue should decide what is hand-written, what is generated, how generated artifacts are reviewed, and which sources are canonical.

### Acceptance Criteria
- The project has an explicit policy for each major reference area: CLI, Python API, configuration, environment variables, and contract or schema reference.
- The policy names a canonical source of truth for each reference area and explains how published pages are refreshed when the source changes.
- Docusaurus integration points for generated or partially generated reference pages are documented and proven by at least one representative reference section.
- The chosen approach avoids duplicating large manually maintained option tables and schema field lists across multiple files.
- Contributor instructions explain how to update reference content as part of normal feature work.
- Developer-facing instructions identify which reference sections are hand-written, which are generated, and which commands or scripts must be run when the canonical source changes.

### Unit tests
- Reference generation or sync checks fail when canonical inputs change without the published artifacts being refreshed.
- At least one representative generated-reference path is exercised in CI.

### Notes/Constraints
- Optimize for maintainability and reviewability, not maximal automation on day one.
- Avoid binding the whole site to a complex documentation toolchain if one small generation step is enough.
- Keep public reference pages readable for humans even when part of the content is generated.
