# Branching model

Janiform uses a simplified Gitflow-style branching model. The goal is to keep production-ready code clear, give active work a stable integration branch, and keep individual changes easy to review.

## Long-lived branches

- `main` contains production-ready code.
- `develop` is the integration branch for active development.

Most work starts from `develop` and is merged back into `develop` through a pull request.

## Topic branches

Use short-lived topic branches for individual pieces of work. Keep the branch focused on one issue, feature, bug, or cleanup.

Common prefixes:

- `feature/` for everything but bug fixes
- `bug/` for bug fixes

Include the issue id when there is one:

```text
feature/jan-439-rework-documentation
bug/jan-512-fix-session-resolution
```

## Normal development flow

1. Start from an up-to-date `develop`.
2. Create a topic branch with the right prefix.
3. Keep commits scoped to the branch purpose.
4. Open a pull request back into `develop`.
5. Delete the topic branch after it merges.

Use `main` as the base only when the work must be applied directly to production-ready code, such as a hotfix.

## Keeping branches focused

A branch should be easy to describe in one sentence. If the work grows into unrelated changes, split it into separate branches or follow-up issues.

Avoid mixing product behavior, documentation rewrites, generated artifacts, and cleanup unless they are part of the same change. Generated files are fine in the same branch when they are produced by the source change being reviewed.

## Updating a branch

Prefer keeping topic branches current with `develop` before review when the branch has drifted or conflicts are likely. Use the smallest update that makes the pull request reviewable.

Do not rewrite shared branches after other people have based work on them unless someone from the core team has agreed to it.
