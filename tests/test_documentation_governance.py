from pathlib import Path


REPOSITORY_ROOT: Path = Path(__file__).resolve().parents[1]
CONTRIBUTING_PATH: Path = REPOSITORY_ROOT / "CONTRIBUTING.md"
DOCUMENTATION_POLICY_PATH: Path = REPOSITORY_ROOT / "documentation_policy.md"
DOCUMENTATION_STYLE_GUIDE_PATH: Path = REPOSITORY_ROOT / "documentation_style_guide.md"
README_PATH: Path = REPOSITORY_ROOT / "README.md"


def test_root_documentation_governance_artifacts_exist() -> None:
    assert CONTRIBUTING_PATH.is_file()
    assert DOCUMENTATION_POLICY_PATH.is_file()
    assert DOCUMENTATION_STYLE_GUIDE_PATH.is_file()


def test_readme_links_to_contributor_entrypoint() -> None:
    readme_text: str = README_PATH.read_text(encoding="utf-8")

    assert "[CONTRIBUTING.md](CONTRIBUTING.md)" in readme_text


def test_contributing_links_root_governance_documents() -> None:
    contributing_text: str = CONTRIBUTING_PATH.read_text(encoding="utf-8")

    assert "[documentation_policy.md](documentation_policy.md)" in contributing_text
    assert "[documentation_style_guide.md](documentation_style_guide.md)" in contributing_text


def test_documentation_policy_covers_required_surfaces_and_locations() -> None:
    policy_text: str = DOCUMENTATION_POLICY_PATH.read_text(encoding="utf-8")

    required_surface_labels: list[str] = [
        "`public`",
        "`contributor`",
        "`internal design`",
        "`planning record`",
        "`generated`",
        "`archive`",
    ]
    required_locations: list[str] = [
        "`README.md`",
        "`docs/`",
        "Local `README.md` files",
        "Generated Markdown",
    ]

    for required_surface_label in required_surface_labels:
        assert required_surface_label in policy_text

    for required_location in required_locations:
        assert required_location in policy_text


def test_documentation_style_guide_covers_required_writer_expectations() -> None:
    style_guide_text: str = DOCUMENTATION_STYLE_GUIDE_PATH.read_text(encoding="utf-8")
    normalized_style_guide_text: str = style_guide_text.lower()

    required_sections: list[str] = [
        "## Tone",
        "## Structure",
        "## Terminology",
        "## Status Labels",
        "## Required Context",
    ]
    required_expectations: list[str] = [
        "assumptions",
        "limitations",
        "determinism",
        "reproducibility",
        "provenance",
    ]

    for required_section in required_sections:
        assert required_section in style_guide_text

    for required_expectation in required_expectations:
        assert required_expectation in normalized_style_guide_text
