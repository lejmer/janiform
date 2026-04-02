import ast
import importlib
import py_compile
import re
from pathlib import Path

from tests.support.public_api_contract_guardrails import (
    PUBLIC_CONTRACT_MODULE_NAMES,
    PUBLIC_PACKAGE_ROOT_MODULE_NAME,
)


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
EXAMPLES_ROOT = REPOSITORY_ROOT / "examples"
DOCUMENT_PATHS = (
    REPOSITORY_ROOT / "README.md",
    REPOSITORY_ROOT / "examples" / "README.md",
    REPOSITORY_ROOT / "docs" / "getting-started" / "quickstart.md",
    REPOSITORY_ROOT / "docs" / "getting-started" / "first-model.md",
)

PYTHON_MODULE_REFERENCE_PATTERN = re.compile(
    r"python -m (examples(?:\.[A-Za-z_][A-Za-z0-9_]*)+)"
)
EXAMPLE_PATH_REFERENCE_PATTERN = re.compile(
    r"examples/[A-Za-z0-9_./-]+\.(?:py|md)"
)


def test_example_modules_are_syntax_valid() -> None:
    for path in example_python_files():
        py_compile.compile(str(path), doraise=True)


def test_example_modules_import_without_execution() -> None:
    for module_name in importable_example_module_names():
        importlib.import_module(module_name)


def test_example_modules_define_main_and_are_guarded() -> None:
    for path in runnable_example_files():
        module_tree = ast.parse(path.read_text(encoding="utf-8"))

        assert any(
            isinstance(statement, ast.FunctionDef) and statement.name == "main"
            for statement in module_tree.body
        ), f"{path} must define main()"
        assert has_dunder_main_guard(module_tree), (
            f"{path} must guard execution with if __name__ == \"__main__\""
        )


def test_example_modules_only_import_public_janiform_modules() -> None:
    allowed_module_names = set(PUBLIC_CONTRACT_MODULE_NAMES)
    allowed_module_names.add(PUBLIC_PACKAGE_ROOT_MODULE_NAME)

    for path in example_python_files():
        module_tree = ast.parse(path.read_text(encoding="utf-8"))

        for statement in ast.walk(module_tree):
            if isinstance(statement, ast.Import):
                for alias in statement.names:
                    if not alias.name.startswith(PUBLIC_PACKAGE_ROOT_MODULE_NAME):
                        continue

                    assert alias.name in allowed_module_names, (
                        f"{path} imports non-public Janiform module {alias.name}"
                    )

            if isinstance(statement, ast.ImportFrom):
                if statement.module is None:
                    continue
                if not statement.module.startswith(PUBLIC_PACKAGE_ROOT_MODULE_NAME):
                    continue

                assert statement.module in allowed_module_names, (
                    f"{path} imports from non-public Janiform module {statement.module}"
                )


def test_example_index_and_docs_reference_existing_examples() -> None:
    for document_path in DOCUMENT_PATHS:
        document_text = document_path.read_text(encoding="utf-8")

        for match in PYTHON_MODULE_REFERENCE_PATTERN.finditer(document_text):
            module_name = match.group(1)
            module_path = REPOSITORY_ROOT.joinpath(*module_name.split(".")).with_suffix(".py")
            assert module_path.exists(), (
                f"{document_path} references missing example module {module_name}"
            )

        for match in EXAMPLE_PATH_REFERENCE_PATTERN.finditer(document_text):
            relative_path = match.group(0)
            assert (REPOSITORY_ROOT / relative_path).exists(), (
                f"{document_path} references missing example path {relative_path}"
            )


def example_python_files() -> list[Path]:
    return sorted(EXAMPLES_ROOT.rglob("*.py"))


def runnable_example_files() -> list[Path]:
    return [
        path
        for path in example_python_files()
        if path.name != "__init__.py" and not path.name.startswith("_")
    ]


def importable_example_module_names() -> list[str]:
    return [path_to_module_name(path) for path in runnable_example_files()]


def path_to_module_name(path: Path) -> str:
    return ".".join(path.relative_to(REPOSITORY_ROOT).with_suffix("").parts)


def has_dunder_main_guard(module_tree: ast.Module) -> bool:
    for statement in module_tree.body:
        if not isinstance(statement, ast.If):
            continue

        test_expression = statement.test
        if not isinstance(test_expression, ast.Compare):
            continue
        if len(test_expression.ops) != 1 or len(test_expression.comparators) != 1:
            continue
        if not isinstance(test_expression.ops[0], ast.Eq):
            continue
        if not isinstance(test_expression.left, ast.Name):
            continue
        if test_expression.left.id != "__name__":
            continue
        comparator = test_expression.comparators[0]
        if not isinstance(comparator, ast.Constant):
            continue
        if comparator.value == "__main__":
            return True

    return False
