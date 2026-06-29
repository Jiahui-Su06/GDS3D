from __future__ import annotations

import gzip
from collections import OrderedDict
from pathlib import Path
from typing import cast

import gdstk
import pyvista as pv
from PySide6.QtWidgets import QVBoxLayout, QWidget
from pyvistaqt import QtInteractor

from i18n import tr
from mesh_factory import make_baseplate_mesh, make_gds_layer_mesh
from objects import BaseplateObject, GdsLayerObject, SceneObject


MeshCacheKey = tuple[str, str, int, int]
MESH_CACHE_COUNT_MAX = 16


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

    def export_png(self, file_path: Path) -> None:
        self.plotter.render()
        self.plotter.screenshot(str(file_path), transparent_background=False)

    def export_svg(self, file_path: Path) -> None:
        self._export_gl2ps(file_path, "svg")
        gz_path = file_path.with_name(file_path.name + ".gz")
        if gz_path.exists():
            with gzip.open(gz_path, "rb") as src, file_path.open("wb") as dst:
                dst.write(src.read())
            gz_path.unlink()

    def export_pdf(self, file_path: Path) -> None:
        self._export_gl2ps(file_path, "pdf")

    def export_gltf(self, file_path: Path) -> None:
        try:
            export_gltf = self.plotter.export_gltf
        except AttributeError:
            export_gltf = None

        if export_gltf is not None:
            export_gltf(str(file_path))
            return

        from vtkmodules.vtkIOExportGLTF import vtkGLTFExporter

        exporter = vtkGLTFExporter()
        exporter.SetRenderWindow(self._render_window())
        exporter.SetFileName(str(file_path))
        if hasattr(exporter, "InlineDataOn"):
            exporter.InlineDataOn()
        exporter.Write()

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
