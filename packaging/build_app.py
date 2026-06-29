from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

from PIL import Image


APP_NAME = "GDS3D"
ROOT = Path(__file__).resolve().parent.parent
SOURCE_ICON = ROOT / "packaging" / "icons" / "icon.png"
GENERATED_ICON_DIR = ROOT / "build" / "icons"


def main() -> int:
    icon_path = _make_app_icon()
    args = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--noconfirm",
        "--clean",
        "--windowed",
        "--onedir",
        "--name",
        APP_NAME,
        "--specpath",
        str(ROOT / "build" / "pyinstaller"),
        "--paths",
        str(ROOT / "src"),
        "--icon",
        str(icon_path),
        "--add-data",
        _data_arg(ROOT / "src" / "industrial.qss", "src"),
        "--add-data",
        _data_arg(ROOT / "src" / "icons", "src/icons"),
        "--add-data",
        _data_arg(ROOT / "i18n", "i18n"),
        "--add-data",
        _data_arg(ROOT / "LICENSE", "."),
        "--collect-submodules",
        "pyvista",
        "--collect-submodules",
        "pyvistaqt",
        "--collect-submodules",
        "vtkmodules",
        str(ROOT / "src" / "main.py"),
    ]
    subprocess.run(args, cwd=ROOT, check=True)
    return 0


def _make_app_icon() -> Path:
    if not SOURCE_ICON.exists():
        raise FileNotFoundError(SOURCE_ICON)

    GENERATED_ICON_DIR.mkdir(parents=True, exist_ok=True)
    if sys.platform == "win32":
        icon_path = GENERATED_ICON_DIR / f"{APP_NAME}.ico"
        _make_ico(icon_path)
        return icon_path
    if sys.platform == "darwin":
        icon_path = GENERATED_ICON_DIR / f"{APP_NAME}.icns"
        _make_icns(icon_path)
        return icon_path
    return SOURCE_ICON


def _make_ico(icon_path: Path) -> None:
    sizes = [(size, size) for size in (16, 24, 32, 48, 64, 128, 256)]
    with Image.open(SOURCE_ICON) as image:
        image.save(icon_path, format="ICO", sizes=sizes)


def _make_icns(icon_path: Path) -> None:
    iconset_dir = GENERATED_ICON_DIR / f"{APP_NAME}.iconset"
    if iconset_dir.exists():
        for path in iconset_dir.iterdir():
            path.unlink()
    else:
        iconset_dir.mkdir(parents=True)

    for size in (16, 32, 128, 256, 512):
        _write_png_icon(iconset_dir / f"icon_{size}x{size}.png", size)
        _write_png_icon(iconset_dir / f"icon_{size}x{size}@2x.png", size * 2)

    subprocess.run(
        ["iconutil", "-c", "icns", str(iconset_dir), "-o", str(icon_path)],
        cwd=ROOT,
        check=True,
    )


def _write_png_icon(path: Path, size: int) -> None:
    with Image.open(SOURCE_ICON) as image:
        resized = image.convert("RGBA").resize((size, size), Image.Resampling.LANCZOS)
        resized.save(path, format="PNG")


def _data_arg(source: Path, destination: str) -> str:
    return f"{source}{os.pathsep}{destination}"


if __name__ == "__main__":
    raise SystemExit(main())
