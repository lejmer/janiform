# Release Notes

Janiform is pre-v0.1 and currently uses a source-first workflow. Release candidates stay inside
GitHub until an external publishing pipeline is introduced explicitly.

## Automated Release Candidate Flow

1. Confirm the branch or commit already contains the intended docs, generated artifacts, and any
   release note entry in `changelogs/v0.x.md`.
2. Run the `release-candidate` GitHub Actions workflow with:
   - `tag_name`: the release candidate tag to create, such as `v0.1.0-rc.1`
   - `target_ref`: the branch or commit to cut from, usually a `release/*` branch that was cut
     from `develop`. If you have not cut the release branch yet, use `develop` explicitly.
3. Let the workflow complete the automated verification set:
   - Linux clean bootstrap, build, Rust tests, Python tests, and generated-artifact checks
   - Windows clean bootstrap, build, Rust tests, and Python tests
   - benchmark governance smoke for the Rust benchmark catalog and baseline/report wiring
   - wheel builds for Linux and Windows
   - SHA-256 checksum generation
   - draft GitHub release creation with the built wheel artifacts attached
4. Run the representative real-data seeded runtime benchmark locally and review its artifacts:

   ```bash
   python3 scripts/python_benchmark_workloads.py --skip-microbenchmark-smoke
   ```

5. Review the draft release notes, the uploaded wheel artifacts, the workflow benchmark artifacts,
   and the local real-data runtime benchmark artifacts before marking the candidate ready.

## Local Dry Run

Use the source-first scripts when you want to rehearse the release candidate path locally before
running the workflow.

```bash
./scripts/bootstrap-linux.sh .venv-linux
./scripts/build-linux.sh .venv-linux
./scripts/test-python-linux.sh .venv-linux
./scripts/test-rust-linux.sh .venv-linux
./.venv-linux/bin/python scripts/generate_crate_readmes.py --check
./.venv-linux/bin/python scripts/generate_third_party_notices.py --check
python3 scripts/python_benchmark_workloads.py
```

The representative real-data seeded runtime benchmark remains local-only.
