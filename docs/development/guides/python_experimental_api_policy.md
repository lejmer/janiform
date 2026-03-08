# Python Experimental API Policy

Document version: `python.experimental_api_policy.v1`

Status: active as of March 8, 2026.

## Purpose

This guide defines how Janiform exposes preview Python endpoints without expanding the
primary public API contract by accident.

Experimental APIs exist so the repo can iterate on importable workflow entrypoints before
promotion into the supported task-oriented modules.

## Namespace and enablement

Preview endpoints must live under the `janiform.experimental` namespace.

- The current preview landing zone is `janiform.experimental.api`
- Preview modules are opt-in behind the environment flag `JANIFORM_EXPERIMENTAL`
- Enabled values are `1`, `true`, `yes`, `on`, `enable`, and `enabled`

Example:

```bash
JANIFORM_EXPERIMENTAL=1 PYTHONPATH=python python3 -c "import janiform.experimental.api"
```

Without the flag, importing a preview endpoint module raises `ExperimentalApiDisabledError`.

## Support boundary

Preview endpoints are not part of the primary public API contract.

- They are not listed in the public API surface map as primary workflow modules
- They are not exported from `janiform`
- They must not be re-exported from `janiform.simulation`, `janiform.pricing`,
  `janiform.risk`, `janiform.calibration`, or other primary public modules
- They are excluded from the approved public contract snapshot and guardrail checks by default
- They are excluded from v0.1 quickstarts and examples unless a specific issue explicitly
  documents experimental usage

## Lifecycle policy

Every preview endpoint must follow one of these paths:

1. Promotion
   - Move the endpoint into a primary task-oriented module
   - Update docs, examples, tests, and public contract guardrails in the same change
   - Remove the experimental entrypoint instead of keeping a compatibility alias
2. Revision
   - Change the preview API directly when correctness or clarity improves
   - Do not preserve compatibility within the experimental namespace
3. Removal
   - Delete the preview endpoint when the experiment is no longer useful

Preview endpoints have no compatibility guarantee until they are promoted into the
primary public API surface.

## Review checklist

- Is the endpoint clearly outside the primary public namespace?
- Is opt-in gating enforced by `JANIFORM_EXPERIMENTAL`?
- Is the preview API absent from root and primary-module `__all__` exports?
- Are primary public contract tests unchanged when previews are disabled?
- If the preview is being promoted, are docs, tests, and guardrails updated together?
