# Release Notes

Janiform is pre-v0.1 and currently uses a source-first workflow. Use this checklist for release candidates, internal cuts, or tag-ready states.

## Checklist

1. Build the project in a clean local environment.

   ```bash
   ./scripts/bootstrap-linux.sh .venv-linux
   ./scripts/build-linux.sh .venv-linux
   ```

2. Run the Python and Rust verification suites that cover the intended release scope.

   ```bash
   ./scripts/test-python-linux.sh .venv-linux
   ./scripts/test-rust-linux.sh .venv-linux
   ```

3. Regenerate or verify generated crate `README.md` files.

   ```bash
   ./.venv-linux/bin/python scripts/generate_crate_readmes.py --check
   ```

4. Confirm the root docs and `docs/` pages match the shipped workflow.

5. Update `changelogs/v0.x.md` if a release note entry is needed for the cut.

The repository does not yet define a published package release pipeline in-repo. Keep release steps procedural and source-oriented until that workflow is introduced explicitly.
