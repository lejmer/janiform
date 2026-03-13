## Python Namespace Policy Reset And Public Submodule Criteria

### Description
Replace the current facade-heavy Python namespace policy with a maintainability-first policy that permits explicit public submodules for real domain areas and limits underscore naming to true internals.

### Implementation details
- Rewrite the Python public-surface and namespace policy docs to remove the assumption that workflow packages should primarily expose one flat root module.
- Define clear criteria for what should be:
  - a curated root re-export
  - a public submodule
  - a private underscore helper
- Explicitly allow multiple supported import paths when one path is a curated convenience import and the other is the symbol's natural domain module.
- State that discoverability and package decomposition are first-class design goals for the Python surface.
- Clarify that private-by-convention naming is not a substitute for coherent package design.
- Mark earlier facade-only guidance as superseded where it conflicts with this project.

### Acceptance Criteria
- The namespace policy document defines public domain submodules as an allowed and recommended part of the Python package structure.
- The policy reserves underscore-prefixed modules for true internals such as low-level helpers, adapters, and boundary plumbing.
- The policy explains how curated root exports and explicit submodule imports coexist without ambiguity.
- The policy no longer treats importability of non-root public domain modules as a design problem by itself.
- The updated guidance is aligned with pre-v0.1 contract evolution rules and assumes direct breaking refactors are allowed.

### Unit tests
- Update public-surface policy tests or snapshot checks so they enforce the new policy rather than the old facade-only assumption.
- Add coverage for at least one package where a symbol is intentionally available from both the curated root and a public domain submodule.

### Notes/Constraints
- This issue supersedes the parts of `JAN-187 Public-API-Surface-Map-And-Namespace-Policy` that force a minimal facade-only structure for workflow packages.
- Keep the policy specific enough to guide refactors, not just descriptive enough to justify whatever structure already exists.
