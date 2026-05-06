# Release process

Releases are built by GitHub Actions from repository state. Do not build or upload release artifacts manually.

The release tag defines the released commit, and CI is the source of truth for artifacts.

## Release branches

We use this branch flow:

```text
feature/bug branches -> develop -> release/vX.Y.Z -> main
main -> develop
```

Feature and bug work lands in `develop`. A release branch is created from `develop` when the release is ready to stabilize.

Only release stabilization work belongs on `release/vX.Y.Z`: versioning, release documentation, generated artifacts, and release-blocking fixes.

## Version source of truth

The release version is defined in the root `Cargo.toml`:

```toml
[workspace.package]
version = "X.Y.Z"
```

Workspace crates inherit this version with `version.workspace = true`. The Python package version is dynamic and is derived by maturin from the PyO3 crate metadata.

For tag releases, the tag must be `vX.Y.Z` and must match the workspace package version.

The version check is implemented in:

```text
scripts/ci/check_release_version.py
```

It validates the root workspace version, workspace crate versions, built wheel metadata when supplied, and installed `janiform.__version__` when requested.

## Release artifacts

The release workflow builds:

- Python wheel for Linux
- Python wheel for Windows
- Python sdist
- standalone Linux CLI archive
- standalone Windows CLI archive
- `SHA256SUMS`
- release manifest JSON

The Python wheel installs the `janiform` command through Python packaging entry points. That command calls the Rust CLI through the PyO3 extension.

The standalone CLI archives contain the Cargo-built `janiform` or `janiform.exe` binary and do not require Python.

## Workflow responsibilities

### Regular PR CI

`.github/workflows/ci-pr-test-rust.yml` runs on pull requests to `develop`.

It owns:

- Rust formatting
- Rust Clippy
- Rust test shards

`.github/workflows/ci-fast-regression.yml` owns the fast deterministic regression tier.

### Generated artifact CI

`.github/workflows/ci-generated.yml` checks generated repository artifacts:

- crate README files
- docs reference pages
- internal crate graph policy
- third-party notices
- bundled presets

### Release-candidate CI

`.github/workflows/ci-release-candidate.yml` runs on:

- pushes to `release/v*`
- pull requests into `main`
- manual dispatch

It owns release-grade checks before a release branch is merged to `main`:

- source version consistency
- Rust formatting and Clippy
- full Rust test shards
- Linux wheel build, content check, install smoke
- Linux sdist build and install smoke
- Linux standalone CLI archive build and smoke
- Windows wheel build, content check, install smoke
- Windows standalone CLI archive build and smoke

### Tag release workflow

`.github/workflows/release.yml` runs on tags matching:

```text
v*.*.*
```

On tag pushes, it validates that the tagged commit is reachable from `main`, builds release artifacts from that exact commit, writes checksums and a manifest, drafts release notes with Release Drafter, and uploads artifacts to the draft GitHub Release.

The workflow can also be run manually as a dry run. Manual runs build and smoke artifacts, write checksums and a manifest, and upload a workflow artifact bundle. They do not create a GitHub Release.

### Release notes

`.github/workflows/release-drafter.yml` updates draft release notes from merged pull requests.

The tag release workflow also runs Release Drafter before uploading artifacts. Release Drafter owns the release body. Artifact upload does not use GitHub's generated release notes.

Use labels that map to `.github/release-drafter.yml`:

- `changelog: breaking`
- `changelog: feature`
- `changelog: fix`
- `changelog: performance`
- `changelog: packaging`
- `changelog: docs`
- `skip-changelog`

Release notes should focus on user-facing changes.
Use `skip-changelog` for CI-only and other maintainer-only pull requests.

## Dry-run testing

Use the release workflow dry run before the first real tag.

In GitHub Actions:

1. Open the `release` workflow.
2. Click `Run workflow`.
3. Enter the current workspace version from `Cargo.toml`.
4. Leave `dry_run` enabled.
5. Run the workflow.

The dry run should upload a `release-dry-run-artifacts` workflow artifact. It must not create a GitHub Release.

If any job exceeds GitHub's time limits, split that job before attempting a real release.

## Cutting a release

1. Start from an up-to-date `develop`.
2. Create the release branch:

   ```bash
   git checkout develop
   git pull
   git checkout -b release/vX.Y.Z
   ```

3. Update `[workspace.package].version` in `Cargo.toml` to `X.Y.Z`.
4. Regenerate release-relevant generated files:

   ```bash
   python scripts/generate_crate_readmes.py
   python scripts/generate_docs_references.py
   python scripts/generate_third_party_notices.py
   ```

   Run other generators when their inputs changed.

5. Push the release branch and let release-candidate CI run:

   ```bash
   git push -u origin release/vX.Y.Z
   ```

6. Open a pull request from `release/vX.Y.Z` into `main`.
7. Wait for release-candidate CI to pass.
8. Merge the release branch into `main`.
9. Tag the merge commit:

   ```bash
   git checkout main
   git pull
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

10. Wait for the `release` workflow to create a draft GitHub Release.
11. Review the draft release notes and uploaded artifacts.
12. Publish the GitHub Release when the draft is correct.
13. Merge `main` back into `develop`:

   ```bash
   git checkout develop
   git pull
   git merge main
   git push origin develop
   ```

## Artifact checks

Wheel content checks live in:

```text
scripts/ci/check_wheel_contents.py
```

Installed package smoke checks live in:

```text
scripts/ci/smoke_installed_wheel.py
```

Standalone CLI archive packaging and smoke checks live in:

```text
scripts/ci/package_cli_archive.py
scripts/ci/smoke_cli_archive.py
```

Checksum and manifest generation live in:

```text
scripts/ci/write_sha256s.py
scripts/ci/build_release_manifest.py
```

## Failure policy

Do not publish a release when any release-candidate or tag release job fails.

If a release workflow is too slow for GitHub Actions, split the job or narrow the release-candidate trigger. Do not skip artifact smoke checks for a tagged release.
