## ISSUE-008: Documentation Style Guide And Review Checklist

### Description
Define a repository documentation style guide and review checklist that raises the prose and structure standard for Janiform. The guidance should cover tone, audience targeting, terminology, page structure, status labeling, limitation disclosure, and rules for finance-facing claims, examples, and assumptions.

### Acceptance Criteria
- A docs style guide exists and is linked from contributor docs.
- The guide defines expected tone, preferred structure, terminology rules, and examples of acceptable versus unacceptable documentation patterns.
- The guide includes requirements for assumptions, limitations, determinism or reproducibility notes, and data or provenance language where relevant to model behavior or outputs.
- A review checklist exists for pull requests that change docs or introduce new documentation surfaces.
- Contributor instructions explain when to update docs and what level of evidence is required for user-facing claims.

### Unit tests
- Repository checks verify the presence of the style guide and review checklist.
- Pull request templates or contributor docs reference the checklist where documentation changes are in scope.

### Notes/Constraints
- Keep the style guide practical and enforceable.
- The goal is disciplined professional writing, not ceremonial process.
