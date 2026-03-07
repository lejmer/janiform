import json
from pathlib import Path

from janiform._api_contract_guardrails import (
    build_public_api_contract_snapshot,
    compute_public_api_contract_snapshot_sha256,
)

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
PUBLIC_API_CONTRACT_SNAPSHOT_PATH = (
    REPOSITORY_ROOT / "tests" / "fixtures" / "python_api_contracts" / "public_api_contract_snapshot.json"
)


def main() -> int:
    snapshot = build_public_api_contract_snapshot()
    snapshot_json = json.dumps(snapshot, indent=2, sort_keys=True) + "\n"

    PUBLIC_API_CONTRACT_SNAPSHOT_PATH.parent.mkdir(parents=True, exist_ok=True)
    PUBLIC_API_CONTRACT_SNAPSHOT_PATH.write_text(snapshot_json, encoding="utf-8")

    print(compute_public_api_contract_snapshot_sha256(snapshot))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
