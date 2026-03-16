from pathlib import Path


def test_benchmark_taxonomy_doc_contains_required_sections() -> None:
    document = _benchmark_taxonomy_document_text()

    required_sections = [
        "## Why Two Layers",
        "## Benchmark Taxonomy",
        "## Criterion",
        "## Pytest Benchmark",
        "## Fixture-Driven Governance",
        "## Local Commands",
        "## CI Policy",
        "## Input Provenance",
    ]
    for required_section in required_sections:
        assert required_section in document


def test_benchmark_taxonomy_doc_contains_required_commands_and_terms() -> None:
    document = _benchmark_taxonomy_document_text()

    required_snippets = [
        "cargo bench -p janiform-core --bench rust_microbenchmarks -- --list",
        "tmpdir=/tmp cargo bench -p janiform-core --bench rust_microbenchmarks",
        "pythonpath=python:. pytest benches/python/python_microbenchmarks.py --benchmark-only",
        (
            "pytest --benchmark-disable benches/python/test_microbenchmark_tooling.py "
            "benches/python/python_microbenchmarks.py -q"
        ),
        "microbenchmark",
        "component benchmark",
        "workflow acceptance benchmark",
        "ci governance benchmark",
    ]
    lowercase_document = document.lower()
    for required_snippet in required_snippets:
        assert required_snippet in lowercase_document
    assert "existing fixture-driven benchmark system" in lowercase_document
    assert "source of truth" in lowercase_document


def test_ci_tiered_execution_strategy_doc_mentions_manual_microbenchmark_smoke() -> None:
    document = _ci_tiered_execution_strategy_document_text()

    assert "Microbenchmark smoke" in document
    assert "cargo bench -p janiform-core --bench rust_microbenchmarks -- --list" in document
    assert (
        "PYTHONPATH=python:. .venv-linux/bin/pytest --benchmark-disable benches/python/test_microbenchmark_tooling.py benches/python/python_microbenchmarks.py -q"
        in document
    )
    assert "manual-only" in document


def _benchmark_taxonomy_document_text() -> str:
    document_path = (
        Path(__file__).resolve().parents[1]
        / "docs"
        / "development"
        / "guides"
        / "benchmark_taxonomy_and_microbenchmark_workflows.md"
    )
    return document_path.read_text(encoding="utf-8")


def _ci_tiered_execution_strategy_document_text() -> str:
    document_path = (
        Path(__file__).resolve().parents[1]
        / "docs"
        / "development"
        / "guides"
        / "ci_tiered_execution_strategy.md"
    )
    return document_path.read_text(encoding="utf-8")
