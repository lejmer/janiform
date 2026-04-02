## ISSUE-001: Documentation Policy, Audience Taxonomy, And Canonical Source Rules

### Description
Create a repository-wide documentation policy that classifies every Markdown artifact by audience and role. The policy should define at minimum the categories `public`, `contributor`, `internal design`, `planning record`, `generated`, and `archive`, and it should explain where each category lives, whether it is published, and which source is canonical when similar content exists in more than one place.

### Acceptance Criteria
- A versioned policy document exists and is easy to find from contributor-facing docs.
- The policy defines audience, publication status, review expectations, and ownership expectations for each documentation category.
- The policy explains where root docs, published docs, crate READMEs, fixture READMEs, planning docs, and generated docs belong.
- The policy defines canonical-source rules so command reference, API reference, workflow examples, and generated material do not drift across multiple competing documents.
- Contributor guidance explains how to decide whether new content belongs in the published docs, root docs, contributor docs, planning, or archive material.

### Unit tests
- Repository checks validate that required policy files exist and are linked from contributor entrypoints.
- Documentation navigation or metadata validation confirms that each primary docs area points back to the policy.

### Notes/Constraints
- Keep the taxonomy small and unambiguous.
- The policy should optimize for long-term maintenance, not just the current rewrite.
