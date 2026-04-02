## Documentation Quality Gates And Executable Examples

### Description
Add verification that prevents the documentation surface from drifting away from the product. The checks should cover at least Markdown hygiene, internal links, public example validity, and synchronization between documented workflows and the supported CLI or Python APIs.

### Acceptance Criteria
- CI includes documentation quality gates such as Markdown linting, internal link checking, and public docs build validation if a published docs platform is used.
- The implementation homes for docs verification are explicit: workflow and CI wiring in `.github/workflows/`, reusable verification scripts in `scripts/ci/`, and any generated docs fixtures or support artifacts in repo-local support directories rather than hidden inside planning.
- Public examples are backed by smoke tests, generated snippets, or executable scripts rather than hand-maintained unverifiable command blocks.
- The repository has a defined policy for how command reference, code snippets, and example outputs stay synchronized with the implementation.
- Docs failures are visible in normal development and release workflows, not only as occasional manual review findings.
- Broken links, empty published pages, and stale example references are treated as quality regressions.

### Validation
- Documentation verification commands run successfully in CI.
- At least one representative market-data calibration path, one simulation path, and one pricing path are exercised by docs-related validation.
- Checks fail when a published page link target is removed or a documented example command drifts from the supported interface.

### Notes/Constraints
- Keep checks targeted enough that they are maintainable.
- Prefer source-backed generation or executable snippets over manually curated long command inventories.
- Verification logic should live with the normal build and test tooling, not in ad hoc manual checklists or project-only notes.
