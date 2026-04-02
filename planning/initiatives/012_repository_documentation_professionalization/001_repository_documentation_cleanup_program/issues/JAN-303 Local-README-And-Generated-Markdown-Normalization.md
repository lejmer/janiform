## Local README And Generated Markdown Normalization

### Description
Normalize repo-adjacent Markdown surfaces that are not part of the public docs site but still affect professionalism and maintainability. This includes crate READMEs, fixture READMEs, examples READMEs, generated docs, and other local Markdown that users or contributors encounter while working in the repository.

### Acceptance Criteria
- The documentation rules for local README classes and generated Markdown notices are defined in the root governance files rather than in `docs/`.
- Each README class has a documented template appropriate to its role, such as crate overview, fixture contract, examples index, or generated artifact notice.
- Generated Markdown files are clearly marked as generated and point to their source-of-truth workflow.
- Crate READMEs are concise, accurate, and aligned with the public repository entrypoints rather than competing with them.
- Fixture and support READMEs state purpose, contract, provenance expectations, and update rules where relevant.

### Unit tests
- Validation checks confirm generated files include generation notices where required.

### Notes/Constraints
- Keep local READMEs role-specific and minimal.
- Do not force every local README into the same template if the content class is different.
- Keep README template rules close to contributor governance, because these files are maintained by contributors across the repo rather than by public-docs authors alone.
