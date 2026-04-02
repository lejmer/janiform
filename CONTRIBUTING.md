# Contributing To Janiform

Use this guide when contributing code, tests, documentation, fixtures, or generated artifacts to Janiform.

## Read First

- [documentation_policy.md](documentation_policy.md)
- [documentation_style_guide.md](documentation_style_guide.md)
- [docs/development/guides/python_style_guide.md](docs/development/guides/python_style_guide.md)

## Development Setup

- Install the repository prerequisites described in [README.md](README.md).
- Bootstrap a local environment with `./scripts/bootstrap-linux.sh` on Linux or `./scripts/bootstrap-win.ps1` on Windows.
- Build with `./scripts/build-linux.sh` or `./scripts/build-win.ps1`.
- Run the relevant Python and Rust test scripts before opening a pull request.

## Change Expectations

- Keep behavior, contracts, examples, and tests aligned in the same change.
- Update or regenerate checked-in generated artifacts when their source of truth changes.
- Prefer shared implementations over local duplication.
- Keep public APIs intentional. If a change is meant to break an in-repo contract, update all in-repo call sites and docs in the same change.

## Where Documentation Belongs

- `README.md` is the first-contact repository entrypoint.
- `docs/` is the canonical user-facing documentation surface.
- Local `README.md` files explain their own directory and link to the canonical docs for broader topics.
- Internal design material should stay off the user-facing docs surface.

## When Documentation Updates Are Required

Update docs in the same change when you modify:

- installation, build, bootstrap, or test instructions
- public CLI or Python workflows
- examples, fixtures, generated Markdown inputs, or directory ownership expectations
- assumptions, limitations, determinism or reproducibility guarantees, or provenance claims

## Verification

- Run the test suites needed to cover both the code paths you changed and the broader behaviors they can affect.
- Run repository checks for generated artifacts or docs when your change touches them.
- Keep Windows and Linux support in mind when editing scripts, paths, or instructions.

## Pull Request Expectations

- Keep the implementation, contracts, fixtures, generated artifacts, and examples aligned.
- Include the tests and checks that demonstrate the change is correct.
- Update affected contributor or user-facing docs when behavior, usage, or operating assumptions changed.
- Regenerate generated Markdown when its source of truth changes.
- Keep the scope intentional. Do not leave duplicate code paths, duplicate documentation paths, or partial in-repo migrations behind.

Documentation-specific review rules, archive handling, and canonical-source expectations live in [documentation_policy.md](documentation_policy.md).

## Release Readiness

A change is not ready to merge or release until the relevant docs are updated and the normal documentation checks pass alongside the code checks.
