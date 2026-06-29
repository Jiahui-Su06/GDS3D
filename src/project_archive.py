from __future__ import annotations

import json
from hashlib import sha1
import zipfile
from dataclasses import asdict, dataclass
from pathlib import Path

from objects import BaseplateObject, Bounds2D, GdsLayerObject, SceneObject


ARCHIVE_FORMAT_VERSION = 1
SCENE_JSON_NAME = "scene.json"
RAW_GDS_DIR = "gds"


@dataclass(frozen=True)
class ProjectArchiveObject:
    kind: str
    payload: dict[str, object]


def write_project_archive(
    file_path: Path, objects: list[SceneObject], gds_source_paths: list[Path]
) -> None:
    path = file_path.expanduser().resolve()
    if path.suffix.lower() != ".gds3d":
        raise ValueError("project archive must use .gds3d extension")

    with zipfile.ZipFile(path, mode="w", compression=zipfile.ZIP_DEFLATED) as zf:
        scene_payload = {
            "format_version": ARCHIVE_FORMAT_VERSION,
            "objects": [_serialize_object(obj) for obj in objects],
        }
        zf.writestr(SCENE_JSON_NAME, json.dumps(scene_payload, indent=2))
        _write_gds_sources(zf, gds_source_paths)


def read_project_archive(file_path: Path) -> tuple[list[ProjectArchiveObject], dict[str, bytes]]:
    path = file_path.expanduser().resolve()
    if path.suffix.lower() != ".gds3d":
        raise ValueError("selected file is not a .gds3d file")

    with zipfile.ZipFile(path) as zf:
        try:
            raw_scene = zf.read(SCENE_JSON_NAME)
        except KeyError as exc:
            raise ValueError("project archive is missing scene.json") from exc

        payload = json.loads(raw_scene)
        if not isinstance(payload, dict):
            raise ValueError("invalid project archive payload")

        objects = payload.get("objects")
        if not isinstance(objects, list):
            raise ValueError("invalid project archive objects")

        archive_objects: list[ProjectArchiveObject] = []
        for item in objects:
            archive_objects.append(_deserialize_object(item))

        gds_sources: dict[str, bytes] = {}
        for name in zf.namelist():
            if not name.startswith(f"{RAW_GDS_DIR}/"):
                continue
            gds_sources[name.removeprefix(f"{RAW_GDS_DIR}/")] = zf.read(name)

    return archive_objects, gds_sources


def _serialize_object(obj: SceneObject) -> dict[str, object]:
    if isinstance(obj, GdsLayerObject):
        payload = {
            "name": obj.name,
            "source_key": obj.source_key or _source_key_for_path(obj.source_path),
            "source_name": obj.source_path.name,
            "cell_name": obj.cell_name,
            "layer": obj.layer,
            "datatype": obj.datatype,
            "display_path": str(obj.file_path),
            "bounds": asdict(obj.bounds),
            "z_min": obj.z_min,
            "z_max": obj.z_max,
            "color": obj.color,
            "brightness": obj.brightness,
            "opacity": obj.opacity,
            "visible": obj.visible,
        }
        return {"kind": obj.kind, "payload": payload}

    if isinstance(obj, BaseplateObject):
        payload = {
            "name": obj.name,
            "bounds": asdict(obj.bounds),
            "z_min": obj.z_min,
            "z_max": obj.z_max,
            "color": obj.color,
            "brightness": obj.brightness,
            "opacity": obj.opacity,
            "visible": obj.visible,
        }
        return {"kind": obj.kind, "payload": payload}

    raise TypeError(f"unsupported object type: {type(obj).__name__}")


def _deserialize_object(item: object) -> ProjectArchiveObject:
    if not isinstance(item, dict):
        raise ValueError("invalid project archive object")
    kind = item.get("kind")
    payload = item.get("payload")
    if not isinstance(kind, str) or not isinstance(payload, dict):
        raise ValueError("invalid project archive object fields")
    return ProjectArchiveObject(kind=kind, payload=payload)


def _write_gds_sources(zf: zipfile.ZipFile, gds_source_paths: list[Path]) -> None:
    seen: set[Path] = set()
    for source in gds_source_paths:
        path = source.expanduser().resolve()
        if path in seen:
            continue
        seen.add(path)
        if not path.exists():
            continue
        zf.write(path, arcname=f"{RAW_GDS_DIR}/{_source_key_for_path(path)}")


def _source_key_for_path(path: Path) -> str:
    resolved = path.expanduser().resolve()
    digest = sha1(str(resolved).encode("utf-8")).hexdigest()[:12]
    return f"{resolved.stem}-{digest}{resolved.suffix.lower()}"
