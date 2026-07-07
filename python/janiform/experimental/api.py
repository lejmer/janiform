"""
Landing zone for preview workflow endpoints.

This module is intentionally excluded from the primary public API contract and is
available only when `JANIFORM_EXPERIMENTAL` is enabled.
"""

from .._experimental import require_experimental_api_enabled


require_experimental_api_enabled(__name__)


__all__ = []
