from __future__ import annotations

import base64
import json
from collections import OrderedDict
from html import escape
from io import BytesIO
from pathlib import Path
from typing import cast

import gdstk
import numpy as np
import pyvista as pv
from PIL import Image
from PySide6.QtWidgets import QVBoxLayout, QWidget
from pyvistaqt import QtInteractor

from i18n import tr
from mesh_factory import make_baseplate_mesh, make_gds_layer_mesh
from objects import BaseplateObject, GdsLayerObject, SceneObject


MeshCacheKey = tuple[str, str, int, int]
MESH_CACHE_COUNT_MAX = 16
SCREENSHOT_PIXEL_COUNT_MAX = 36_000_000
GLTF_ARRAY_BUFFER = 34962
GLTF_ELEMENT_ARRAY_BUFFER = 34963
GLTF_FLOAT = 5126
GLTF_UNSIGNED_INT = 5125
GLTF_TRIANGLES = 4


class Viewport(QWidget):
    """Qt widget that owns the PyVista scene."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.plotter = QtInteractor(self)
        self._actors: dict[str, object] = {}
        self._mesh_cache: OrderedDict[MeshCacheKey, pv.PolyData] = OrderedDict()
        self._selection_actor: object | None = None
        self._axes_visible = True

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.plotter)

        self.plotter.set_background("#FFFFFF")
        self.plotter.add_axes()

    def set_axes_visible(self, visible: bool) -> None:
        if self._axes_visible == visible:
            return

        self._axes_visible = visible
        if visible:
            self.plotter.show_axes()
        else:
            self.plotter.hide_axes()
        self.plotter.render()

    def add_or_update(
        self,
        obj: SceneObject,
        polygons: list[gdstk.Polygon] | None = None,
        cache_key: MeshCacheKey | None = None,
    ) -> None:
        mesh = self._get_mesh(obj, polygons, cache_key)
        self._add_actor(obj, mesh)

    def _add_actor(self, obj: SceneObject, mesh: pv.PolyData) -> None:
        self.remove_object(obj.id)
        actor = self.plotter.add_mesh(
            mesh,
            color=obj.color,
            opacity=obj.opacity,
            show_edges=False,
            ambient=0.55,
            name=obj.id,
            reset_camera=False,
        )
        self._actors[obj.id] = actor
        self.update_actor(obj, render=False)
        self.plotter.render()

    def _get_mesh(
        self,
        obj: SceneObject,
        polygons: list[gdstk.Polygon] | None,
        cache_key: MeshCacheKey | None,
    ) -> pv.PolyData:
        if cache_key is None:
            return self._make_mesh(obj, polygons)

        mesh = self._mesh_cache.get(cache_key)
        if mesh is not None:
            self._mesh_cache.move_to_end(cache_key)
            return mesh

        mesh = self._make_mesh(obj, polygons)
        self._mesh_cache[cache_key] = mesh
        if len(self._mesh_cache) > MESH_CACHE_COUNT_MAX:
            self._mesh_cache.popitem(last=False)
        return mesh

    def update_actor(self, obj: SceneObject, render: bool = True) -> None:
        actor = self._actors.get(obj.id)
        if actor is None:
            return

        actor.SetVisibility(obj.visible)
        actor.SetPosition(0.0, 0.0, obj.z_min)
        actor.SetScale(1.0, 1.0, obj.z_max - obj.z_min)

        prop = actor.GetProperty()
        prop.SetColor(_lit_rgb_from_hex(obj.color, obj.brightness))
        prop.SetOpacity(obj.opacity)

        if render:
            self.plotter.render()

    def rebuild_geometry(
        self,
        obj: SceneObject,
        polygons: list[gdstk.Polygon] | None = None,
        cache_key: MeshCacheKey | None = None,
    ) -> None:
        self.add_or_update(obj, polygons, cache_key)

    def set_object_visible(self, object_id: str, visible: bool) -> None:
        actor = self._actors.get(object_id)
        if actor is not None:
            actor.SetVisibility(visible)
        self.plotter.render()

    def remove_object(self, object_id: str) -> None:
        actor = self._actors.pop(object_id, None)
        if actor is not None:
            self.plotter.remove_actor(actor, reset_camera=False)
        if self._selection_actor is not None:
            self.plotter.remove_actor(self._selection_actor, reset_camera=False)
            self._selection_actor = None
        self.plotter.render()

    def clear_scene(self) -> None:
        for actor in list(self._actors.values()):
            self.plotter.remove_actor(actor, reset_camera=False)
        self._actors.clear()
        if self._selection_actor is not None:
            self.plotter.remove_actor(self._selection_actor, reset_camera=False)
            self._selection_actor = None
        self.plotter.render()

    def highlight_object(self, object_id: str | None) -> None:
        if self._selection_actor is not None:
            self.plotter.remove_actor(self._selection_actor, reset_camera=False)
            self._selection_actor = None

        if object_id is None:
            self.plotter.render()
            return

        actor = self._actors.get(object_id)
        if actor is None:
            self.plotter.render()
            return

        bounds = actor.GetBounds()
        self._highlight_bounds(
            cast(tuple[float, float, float, float, float, float], bounds),
            f"{object_id}-selection",
        )
        self.plotter.render()

    def highlight_objects(self, object_ids: list[str]) -> None:
        if self._selection_actor is not None:
            self.plotter.remove_actor(self._selection_actor, reset_camera=False)
            self._selection_actor = None

        bounds_list: list[tuple[float, float, float, float, float, float]] = []
        for object_id in object_ids:
            actor = self._actors.get(object_id)
            if actor is None:
                continue
            bounds_list.append(
                cast(tuple[float, float, float, float, float, float], actor.GetBounds())
            )

        if not bounds_list:
            self.plotter.render()
            return

        self._highlight_bounds(_merge_actor_bounds(bounds_list), "group-selection")
        self.plotter.render()

    def _highlight_bounds(
        self,
        bounds: tuple[float, float, float, float, float, float],
        name: str,
    ) -> None:
        outline = pv.Box(bounds=bounds)
        self._selection_actor = self.plotter.add_mesh(
            outline,
            color="#F0B429",
            style="wireframe",
            line_width=2,
            name=name,
            reset_camera=False,
        )

    def reset_camera(self) -> None:
        self.plotter.reset_camera()
        self.plotter.render()

    def export_png(
        self,
        file_path: Path,
        image_size: tuple[int, int] | None = None,
    ) -> None:
        self.plotter.render()
        safe_size = self._safe_image_size(image_size)
        if safe_size is None:
            self.plotter.screenshot(
                str(file_path),
                transparent_background=False,
            )
            return

        image = self._capture_image_on_canvas(safe_size)
        Image.fromarray(image).save(file_path)

    def export_svg(
        self,
        file_path: Path,
        image_size: tuple[int, int] | None = None,
    ) -> None:
        self.plotter.render()
        safe_size = self._safe_image_size(image_size)
        image_data = BytesIO()
        if safe_size is None:
            image = self.plotter.screenshot(
                filename=image_data,
                transparent_background=False,
            )
        else:
            image = self._capture_image_on_canvas(safe_size)
            Image.fromarray(image).save(image_data, format="PNG")
        if image is None:
            raise RuntimeError(tr("error.svg_screenshot_failed"))

        width = int(image.shape[1])
        height = int(image.shape[0])
        encoded = base64.b64encode(image_data.getvalue()).decode("ascii")
        title = escape(file_path.stem)
        svg = (
            f'<svg xmlns="http://www.w3.org/2000/svg" '
            f'xmlns:xlink="http://www.w3.org/1999/xlink" '
            f'width="{width}" height="{height}" viewBox="0 0 {width} {height}">\n'
            f"  <title>{title}</title>\n"
            f'  <image width="{width}" height="{height}" '
            f'href="data:image/png;base64,{encoded}" '
            f'xlink:href="data:image/png;base64,{encoded}"/>\n'
            f"</svg>\n"
        )
        file_path.write_text(svg, encoding="utf-8")

    def export_pdf(self, file_path: Path) -> None:
        self._export_gl2ps(file_path, "pdf")

    def export_gltf(self, file_path: Path) -> None:
        selection_actor = self._selection_actor
        selection_visible = _actor_visibility(selection_actor)
        if selection_actor is not None:
            selection_actor.SetVisibility(False)
            self.plotter.render()

        try:
            self._export_gltf(file_path)
        finally:
            if selection_actor is not None and selection_actor is self._selection_actor:
                selection_actor.SetVisibility(selection_visible)
                self.plotter.render()

    def _export_gltf(self, file_path: Path) -> None:
        objects = []
        for object_id, actor in self._actors.items():
            if not _actor_visibility(actor):
                continue

            mesh = _actor_mesh(actor)
            positions, indices = _gltf_geometry(mesh, actor)
            if len(positions) == 0 or len(indices) == 0:
                continue

            objects.append(
                {
                    "name": object_id,
                    "positions": positions,
                    "indices": indices,
                    "color": _actor_color(actor),
                }
            )

        if not objects:
            raise RuntimeError(tr("error.gltf_no_visible_objects"))

        file_path.write_text(_gltf_document(objects), encoding="utf-8")

    def closeEvent(self, event) -> None:  # noqa: N802
        self.plotter.close()
        super().closeEvent(event)

    def _make_mesh(
        self,
        obj: SceneObject,
        polygons: list[gdstk.Polygon] | None,
    ) -> pv.PolyData:
        if isinstance(obj, BaseplateObject):
            return make_baseplate_mesh(obj)
        if isinstance(obj, GdsLayerObject):
            if polygons is None:
                raise ValueError(tr("error.gds_object_requires_polygons"))
            return make_gds_layer_mesh(obj, polygons)
        raise TypeError(tr("error.unsupported_object_type", name=type(obj).__name__))

    def _render_window(self):
        render_window = getattr(self.plotter, "ren_win", None)
        if render_window is None:
            render_window = getattr(self.plotter, "render_window", None)
        if render_window is None:
            raise RuntimeError(tr("error.render_window_unavailable"))
        return render_window

    def _safe_image_size(
        self,
        requested_size: tuple[int, int] | None,
    ) -> tuple[int, int] | None:
        if requested_size is None:
            return None

        width = max(1, int(requested_size[0]))
        height = max(1, int(requested_size[1]))
        pixel_count = width * height
        if pixel_count <= SCREENSHOT_PIXEL_COUNT_MAX:
            return (width, height)

        ratio = (SCREENSHOT_PIXEL_COUNT_MAX / pixel_count) ** 0.5
        safe_width = max(1, int(width * ratio))
        safe_height = max(1, int(height * ratio))
        return (safe_width, safe_height)

    def _capture_image_on_canvas(self, canvas_size: tuple[int, int]) -> np.ndarray:
        canvas_width, canvas_height = canvas_size
        capture_size = self._capture_size_for_canvas(canvas_size)
        image = self.plotter.screenshot(
            filename=None,
            transparent_background=False,
            window_size=capture_size,
        )
        if image is None:
            raise RuntimeError(tr("error.svg_screenshot_failed"))

        image = _rgb_image(image)
        image_height, image_width = image.shape[:2]
        canvas = np.full((canvas_height, canvas_width, 3), 255, dtype=np.uint8)
        x_offset = (canvas_width - image_width) // 2
        y_offset = (canvas_height - image_height) // 2
        canvas[
            y_offset : y_offset + image_height,
            x_offset : x_offset + image_width,
        ] = image
        return canvas

    def _capture_size_for_canvas(
        self,
        canvas_size: tuple[int, int],
    ) -> tuple[int, int]:
        canvas_width, canvas_height = canvas_size
        viewport_width, viewport_height = self.plotter.window_size
        viewport_width = max(1, int(viewport_width))
        viewport_height = max(1, int(viewport_height))
        viewport_ratio = viewport_width / viewport_height
        canvas_ratio = canvas_width / canvas_height

        if viewport_ratio >= canvas_ratio:
            capture_width = canvas_width
            capture_height = max(1, round(capture_width / viewport_ratio))
        else:
            capture_height = canvas_height
            capture_width = max(1, round(capture_height * viewport_ratio))
        return (capture_width, capture_height)

    def _export_gl2ps(self, file_path: Path, file_format: str) -> None:
        from vtkmodules.vtkIOExportGL2PS import vtkGL2PSExporter

        prefix = file_path.with_suffix("")
        exporter = vtkGL2PSExporter()
        exporter.SetRenderWindow(self._render_window())
        exporter.SetFilePrefix(str(prefix))
        if file_format == "svg":
            exporter.SetFileFormatToSVG()
        elif file_format == "pdf":
            exporter.SetFileFormatToPDF()
        else:
            raise ValueError(tr("error.unsupported_export_format", format=file_format))
        exporter.Write()


def _rgb_from_hex(value: str) -> tuple[float, float, float]:
    color = value.lstrip("#")
    if len(color) != 6:
        raise ValueError(tr("error.invalid_color", value=value))
    red = int(color[0:2], 16) / 255.0
    green = int(color[2:4], 16) / 255.0
    blue = int(color[4:6], 16) / 255.0
    return (red, green, blue)


def _lit_rgb_from_hex(value: str, brightness: float) -> tuple[float, float, float]:
    if not 0.0 <= brightness <= 2.0:
        raise ValueError(tr("error.brightness_range"))
    return tuple(min(channel * brightness, 1.0) for channel in _rgb_from_hex(value))


def _actor_visibility(actor: object | None) -> bool:
    if actor is None or not hasattr(actor, "GetVisibility"):
        return False
    return bool(actor.GetVisibility())


def _actor_mesh(actor: object) -> pv.PolyData:
    mapper = actor.GetMapper()
    if mapper is None:
        raise RuntimeError(tr("error.gltf_actor_mesh_unavailable"))
    data = mapper.GetInput()
    if data is None:
        raise RuntimeError(tr("error.gltf_actor_mesh_unavailable"))
    mesh = pv.wrap(data)
    if not isinstance(mesh, pv.PolyData):
        mesh = mesh.extract_surface(algorithm="dataset_surface")
    else:
        mesh = mesh.copy(deep=True)
    _clear_mesh_data(mesh)
    mesh = mesh.triangulate()
    _clear_mesh_data(mesh)
    return cast(pv.PolyData, mesh)


def _actor_color(actor: object) -> tuple[float, float, float, float]:
    prop = actor.GetProperty()
    red, green, blue = prop.GetColor()
    return (float(red), float(green), float(blue), float(prop.GetOpacity()))


def _clear_mesh_data(mesh: pv.PolyData) -> None:
    for name in list(mesh.point_data):
        del mesh.point_data[name]
    for name in list(mesh.cell_data):
        del mesh.cell_data[name]
    for name in list(mesh.field_data):
        del mesh.field_data[name]


def _gltf_geometry(
    mesh: pv.PolyData,
    actor: object,
) -> tuple[np.ndarray, np.ndarray]:
    points = np.asarray(mesh.points, dtype=np.float32)
    faces = np.asarray(mesh.faces, dtype=np.uint32)
    if len(points) == 0 or len(faces) == 0:
        return (
            np.empty((0, 3), dtype=np.float32),
            np.empty(0, dtype=np.uint32),
        )

    faces = faces.reshape((-1, 4))
    triangles = faces[faces[:, 0] == 3][:, 1:].reshape(-1)
    if len(triangles) == 0:
        return (
            np.empty((0, 3), dtype=np.float32),
            np.empty(0, dtype=np.uint32),
        )

    scale = np.asarray(actor.GetScale(), dtype=np.float32)
    position = np.asarray(actor.GetPosition(), dtype=np.float32)
    transformed = points * scale + position
    gltf_points = np.column_stack(
        (
            transformed[:, 0],
            transformed[:, 2],
            -transformed[:, 1],
        )
    ).astype(np.float32)
    return (gltf_points, triangles.astype(np.uint32))


def _gltf_document(objects: list[dict[str, object]]) -> str:
    buffer = bytearray()
    buffer_views = []
    accessors = []
    meshes = []
    nodes = []
    materials = []

    for index, obj in enumerate(objects):
        positions = cast(np.ndarray, obj["positions"])
        indices = cast(np.ndarray, obj["indices"])

        position_view = _append_buffer_view(buffer, positions, GLTF_ARRAY_BUFFER)
        buffer_views.append(position_view)
        accessors.append(
            _accessor(
                buffer_view_index=len(buffer_views) - 1,
                component_type=GLTF_FLOAT,
                count=len(positions),
                accessor_type="VEC3",
                minimum=positions.min(axis=0).tolist(),
                maximum=positions.max(axis=0).tolist(),
            )
        )
        position_accessor = len(accessors) - 1

        index_view = _append_buffer_view(buffer, indices, GLTF_ELEMENT_ARRAY_BUFFER)
        buffer_views.append(index_view)
        accessors.append(
            _accessor(
                buffer_view_index=len(buffer_views) - 1,
                component_type=GLTF_UNSIGNED_INT,
                count=len(indices),
                accessor_type="SCALAR",
                minimum=[int(indices.min())],
                maximum=[int(indices.max())],
            )
        )
        index_accessor = len(accessors) - 1

        materials.append(_material(cast(tuple[float, float, float, float], obj["color"])))
        meshes.append(
            {
                "name": str(obj["name"]),
                "primitives": [
                    {
                        "attributes": {"POSITION": position_accessor},
                        "indices": index_accessor,
                        "material": index,
                        "mode": GLTF_TRIANGLES,
                    }
                ],
            }
        )
        nodes.append({"name": str(obj["name"]), "mesh": index})

    payload = {
        "asset": {"version": "2.0", "generator": "GDS3D"},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "buffers": [
            {
                "byteLength": len(buffer),
                "uri": "data:application/octet-stream;base64,"
                + base64.b64encode(buffer).decode("ascii"),
            }
        ],
        "bufferViews": buffer_views,
        "accessors": accessors,
    }
    return json.dumps(payload, ensure_ascii=False, separators=(",", ":"))


def _append_buffer_view(
    buffer: bytearray,
    array: np.ndarray,
    target: int,
) -> dict[str, int]:
    while len(buffer) % 4:
        buffer.append(0)
    byte_offset = len(buffer)
    contiguous = np.ascontiguousarray(array)
    raw = contiguous.tobytes()
    buffer.extend(raw)
    return {
        "buffer": 0,
        "byteOffset": byte_offset,
        "byteLength": len(raw),
        "target": target,
    }


def _accessor(
    buffer_view_index: int,
    component_type: int,
    count: int,
    accessor_type: str,
    minimum: list[float] | list[int],
    maximum: list[float] | list[int],
) -> dict[str, object]:
    return {
        "bufferView": buffer_view_index,
        "byteOffset": 0,
        "componentType": component_type,
        "count": count,
        "type": accessor_type,
        "min": minimum,
        "max": maximum,
    }


def _material(color: tuple[float, float, float, float]) -> dict[str, object]:
    material: dict[str, object] = {
        "pbrMetallicRoughness": {
            "baseColorFactor": list(color),
            "metallicFactor": 0.0,
            "roughnessFactor": 0.9,
        },
        "doubleSided": True,
    }
    if color[3] < 1.0:
        material["alphaMode"] = "BLEND"
    return material


def _rgb_image(image: np.ndarray) -> np.ndarray:
    if image.ndim != 3:
        raise RuntimeError(tr("error.svg_screenshot_failed"))

    if image.shape[2] >= 3:
        rgb = image[:, :, :3]
    else:
        raise RuntimeError(tr("error.svg_screenshot_failed"))

    if rgb.dtype == np.uint8:
        return rgb

    if np.issubdtype(rgb.dtype, np.floating):
        return np.clip(rgb * 255.0, 0, 255).astype(np.uint8)
    return np.clip(rgb, 0, 255).astype(np.uint8)


def _merge_actor_bounds(
    bounds_list: list[tuple[float, float, float, float, float, float]],
) -> tuple[float, float, float, float, float, float]:
    if not bounds_list:
        raise ValueError(tr("error.bounds_empty"))

    return (
        min(bounds[0] for bounds in bounds_list),
        max(bounds[1] for bounds in bounds_list),
        min(bounds[2] for bounds in bounds_list),
        max(bounds[3] for bounds in bounds_list),
        min(bounds[4] for bounds in bounds_list),
        max(bounds[5] for bounds in bounds_list),
    )
