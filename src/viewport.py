from __future__ import annotations

from collections import OrderedDict
from typing import cast

import gdstk
import pyvista as pv
from PySide6.QtWidgets import QVBoxLayout, QWidget
from pyvistaqt import QtInteractor

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

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.plotter)

        self.plotter.set_background("#FFFFFF")
        self.plotter.add_axes()

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
        outline = pv.Box(
            bounds=cast(tuple[float, float, float, float, float, float], bounds)
        )
        self._selection_actor = self.plotter.add_mesh(
            outline,
            color="#F0B429",
            style="wireframe",
            line_width=2,
            name=f"{object_id}-selection",
            reset_camera=False,
        )
        self.plotter.render()

    def reset_camera(self) -> None:
        self.plotter.reset_camera()
        self.plotter.render()

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
                raise ValueError("GDS object requires polygon data")
            return make_gds_layer_mesh(obj, polygons)
        raise TypeError(f"unsupported object type: {type(obj).__name__}")


def _rgb_from_hex(value: str) -> tuple[float, float, float]:
    color = value.lstrip("#")
    if len(color) != 6:
        raise ValueError(f"invalid color: {value}")
    red = int(color[0:2], 16) / 255.0
    green = int(color[2:4], 16) / 255.0
    blue = int(color[4:6], 16) / 255.0
    return (red, green, blue)


def _lit_rgb_from_hex(value: str, brightness: float) -> tuple[float, float, float]:
    if not 0.0 <= brightness <= 2.0:
        raise ValueError("brightness must be between 0 and 2")
    return tuple(min(channel * brightness, 1.0) for channel in _rgb_from_hex(value))
