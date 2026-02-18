from pathlib import Path


def test_validation_fallback_policy_doc_contains_required_sections() -> None:
    policy_document = _validation_policy_document_text()

    required_sections: list[str] = [
        "## Policy Scope",
        "## Policy Contract",
        "## Unknown Date Behavior",
        "## Unsupported Exchange Calendar Segment Behavior",
        "## Incomplete Fixture Coverage Behavior",
        "## Failure Modes",
        "## Triage Steps",
        "## Examples",
    ]
    for required_section in required_sections:
        assert required_section in policy_document


def test_validation_fallback_policy_doc_contains_unknown_date_and_incomplete_fixture_examples() -> None:
    policy_document = _validation_policy_document_text().lower()

    assert "unknown-date example" in policy_document
    assert "incomplete-fixture example" in policy_document


def _validation_policy_document_text() -> str:
    policy_document_path: Path = (
        Path(__file__).resolve().parents[1]
        / "docs"
        / "development"
        / "guides"
        / "validation_fallback_policy.md"
    )
    return policy_document_path.read_text(encoding="utf-8")
