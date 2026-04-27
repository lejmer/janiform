from ..errors import calibration_error


def preset_registry_error(field_path: str, message: str) -> Exception:
    return calibration_error(
        field_path,
        message,
        entrypoint="preset_registry",
    )
