import ast
from pathlib import Path

from janiform._native_api import native_error_parts


REPO_ROOT = Path(__file__).resolve().parent.parent
PYTHON_SOURCE_ROOT = REPO_ROOT / "python" / "janiform"
BENCH_SOURCE_ROOT = REPO_ROOT / "benches" / "python"


def test_only_native_api_imports_private_native_module() -> None:
    violating_files: list[str] = []

    for source_root in (PYTHON_SOURCE_ROOT, BENCH_SOURCE_ROOT):
        for path in sorted(source_root.rglob("*.py")):
            if path == PYTHON_SOURCE_ROOT / "_native_api.py":
                continue

            module_tree = ast.parse(path.read_text(encoding="utf-8"))

            if _module_imports_private_native_module(module_tree):
                violating_files.append(str(path.relative_to(REPO_ROOT)))

    assert violating_files == [], (
        "Direct imports from janiform._native are only allowed in python/janiform/_native_api.py: "
        f"{violating_files}"
    )


def test_type_checking_imports_used_in_annotations_require_deferred_annotations() -> None:
    violating_files: list[str] = []

    for source_root in (PYTHON_SOURCE_ROOT, BENCH_SOURCE_ROOT):
        for path in sorted(source_root.rglob("*.py")):
            module_tree = ast.parse(path.read_text(encoding="utf-8"))
            if _has_future_annotations_import(module_tree):
                continue

            type_checking_names = _collect_type_checking_imported_names(module_tree)
            if not type_checking_names:
                continue

            if _module_uses_names_in_annotations(module_tree, type_checking_names):
                violating_files.append(str(path.relative_to(REPO_ROOT)))

    assert violating_files == [], (
        "Files that use TYPE_CHECKING-only imports in runtime annotations must enable "
        f"deferred annotations: {violating_files}"
    )


def test_native_error_parts_preserves_direct_field_path() -> None:
    assert native_error_parts(
        "session_configuration: must define a session",
        default_field_path="request",
    ) == ("session_configuration", "must define a session")


def test_native_error_parts_preserves_direct_legacy_display_field_path() -> None:
    assert native_error_parts(
        "preset artifact: must be a JSON object",
        default_field_path="preset_resolution",
    ) == ("preset artifact", "must be a JSON object")


def test_native_error_parts_preserves_wrapped_field_path() -> None:
    assert native_error_parts(
        "failed to resolve market-data session: session_configuration: must define a session",
        default_field_path="request",
    ) == ("session_configuration", "must define a session")


def _module_imports_private_native_module(module_tree: ast.Module) -> bool:
    for node in ast.walk(module_tree):
        if isinstance(node, ast.Import):
            if any(alias.name == "janiform._native" for alias in node.names):
                return True
        if isinstance(node, ast.ImportFrom):
            if node.module in {"_native", "janiform._native"}:
                return True

    return False


def _has_future_annotations_import(module_tree: ast.Module) -> bool:
    return any(
        isinstance(node, ast.ImportFrom)
        and node.module == "__future__"
        and any(alias.name == "annotations" for alias in node.names)
        for node in module_tree.body
    )


def _collect_type_checking_imported_names(module_tree: ast.Module) -> set[str]:
    imported_names: set[str] = set()

    for node in module_tree.body:
        if not isinstance(node, ast.If):
            continue
        if not isinstance(node.test, ast.Name) or node.test.id != "TYPE_CHECKING":
            continue

        for child in node.body:
            match child:
                case ast.Import():
                    for alias in child.names:
                        imported_names.add(alias.asname or alias.name.rsplit(".", maxsplit=1)[-1])
                case ast.ImportFrom():
                    for alias in child.names:
                        imported_names.add(alias.asname or alias.name)

    return imported_names


def _module_uses_names_in_annotations(
    module_tree: ast.Module,
    type_checking_names: set[str],
) -> bool:
    for node in ast.walk(module_tree):
        match node:
            case ast.FunctionDef() | ast.AsyncFunctionDef():
                annotation_nodes = [
                    argument.annotation
                    for argument in (
                        list(node.args.posonlyargs)
                        + list(node.args.args)
                        + list(node.args.kwonlyargs)
                        + ([node.args.vararg] if node.args.vararg is not None else [])
                        + ([node.args.kwarg] if node.args.kwarg is not None else [])
                    )
                    if argument is not None and argument.annotation is not None
                ]
                if node.returns is not None:
                    annotation_nodes.append(node.returns)
            case ast.AnnAssign():
                annotation_nodes = [node.annotation]
            case _:
                continue

        for annotation in annotation_nodes:
            if _annotation_references_type_checking_name(annotation, type_checking_names):
                return True

    return False


def _annotation_references_type_checking_name(
    annotation: ast.expr,
    type_checking_names: set[str],
) -> bool:
    return any(
        isinstance(node, ast.Name) and node.id in type_checking_names
        for node in ast.walk(annotation)
    )
