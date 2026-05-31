# Contribution guide

These pages explain how to work in the Janiform repository. Start with setup, then use the language and workflow guides that match the change you are making.

## Recommended path

1. [Set up a development environment](setup.md)
2. [Repository layout](repo-layout.md)
3. [General conventions](general-conventions.md)
4. [Maintaining public API contracts](public-api-contracts.md)
5. [Writing tests](writing-tests.md)
6. [Writing a pull request](writing-a-pr.md)

## Guides

| Page                                            | Use it for                                                                               |
|-------------------------------------------------|------------------------------------------------------------------------------------------|
| [Set up a development environment](setup.md)    | Bootstrapping the local Python and Rust development environment.                         |
| [Repository layout](repo-layout.md)             | Understanding the top-level repository folders.                                          |
| [General conventions](general-conventions.md)   | Cross-cutting project rules that apply across languages.                                 |
| [Public API contracts](public-api-contracts.md) | Public API policy, guardrails, snapshot updates, and approval manifests.                 |
| [Python conventions](python-conventions.md)     | Python boundary-layer style, imports, type hints, public contracts, and tests.           |
| [Rust conventions](rust-conventions.md)         | Rust style, APIs, errors, PyO3 boundaries, concurrency, and tests.                       |
| [Rust crates](crates.md)                        | Crate ownership, workspace structure, dependency direction, and adding or moving crates. |
| [Generation scripts](generation-scripts.md)     | Generated repository artifacts, when to regenerate them, and check-mode scripts.         |
| [Writing tests](writing-tests.md)               | Python and Rust test expectations, integration tests, regression tests, and benchmarks.  |
| [Branching model](branching-model.md)           | Branch naming, long-lived branches, and the simplified Gitflow model.                    |
| [Writing a pull request](writing-a-pr.md)       | PR scope, title format, body sections, verification, and generated files.                |
