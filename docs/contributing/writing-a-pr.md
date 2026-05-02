# Writing a pull request

A pull request should make the intended change easy to review. Keep the branch focused, describe the behavior clearly, and include the verification steps that supports the change exactly as you ran them.

For branch naming and target branches, see [Branching model](branching-model.md).

## Scope

Open a pull request for one coherent change. A good PR can usually be summarized in one sentence.

Keep unrelated cleanup, generated artifact refreshes, documentation rewrites, and product behavior changes separate unless they are part of the same contract change. Generated files belong in the same PR when they are produced by the source change under review.

## Title

Use a Conventional Commits-style PR title:

```text
<type>(optional-scope): short imperative summary
```

Rules:

- keep the title lower-case
- use present tense and imperative mood
- keep it concise, roughly 72 characters or less
- do not end with a period

Common types:

- `feat` for new user-facing behavior
- `fix` for bug fixes
- `docs` for documentation-only changes
- `refactor` for internal restructuring without behavior changes
- `test` for test-only changes
- `chore` for maintenance

Examples:

```text
docs(contributing): add pull request guide
fix(calibration): infer market profile from synthetic tick paths
refactor(workflows): move fit preparation into shared runtime
```

## Description

Use two sections in the PR body:

```markdown
## Change Summary

- ...

## Related Issue

- [#439](https://github.com/lejmer/janiform/issues/439)
```

The change summary should describe what the PR proposes, not narrate the work process. Use bullets that are specific enough for a reviewer to understand the behavioral surface.

The related issue section should link the GitHub issue when one exists. Render the link as the issue number.

## Verification

List the commands you ran and the result. Include focused tests when the change is narrow, and broader tests when the change touches shared behavior, contracts, workflows, or generated artifacts.

If you did not run a relevant check, say so and explain why. Do not imply that CI or another reviewer has covered something you did not verify.

For test expectations, see [Writing tests](writing-tests.md).

## Generated files

Generated files should be reviewed, not ignored. If the PR includes generated output, mention the generator in the change summary or verification.

Run the matching `--check` command when the generator supports one. For the generator list, see [Generation scripts](generation-scripts.md).

## Drafts and review

Open a draft PR when the shape is useful to share but the work is not ready for final review.

Mark the PR ready when:

- the scope is coherent
- generated files are current
- relevant docs are updated
- verification is included
- known follow-up work is either handled or captured in an issue
