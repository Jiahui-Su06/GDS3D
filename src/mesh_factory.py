from __future__ import annotations

from typing import cast

import gdstk
import mapbox_earcut as earcut
import numpy as np
import pyvista as pv

from i18n import tr
from objects import BaseplateObject, GdsLayerObject


def make_baseplate_mesh(obj: BaseplateObject) -> pv.PolyData:
    bounds = obj.bounds
    return pv.Box(
        bounds=(
            bounds.min_x,
            bounds.max_x,
            bounds.min_y,
            bounds.max_y,
            0.0,
            1.0,
        )
    )


def make_gds_layer_mesh(
    obj: GdsLayerObject, polygons: list[gdstk.Polygon]
) -> pv.PolyData:
    if not polygons:
        raise ValueError(tr("error.gds_mesh_requires_polygons"))

    source_polygons = _union_polygons(polygons)
    meshes: list[pv.PolyData] = []
    for poly in source_polygons:
        xy = np.asarray(poly.points, dtype=np.float64)
        try:
            meshes.append(_extrude_polygon(xy))
        except ValueError:
            continue

    if not meshes:
        raise ValueError(tr("error.no_valid_polygons_mesh"))

    mesh = meshes[0]
    for part in meshes[1:]:
        mesh = cast(pv.PolyData, mesh.merge(part))
    return cast(pv.PolyData, mesh.clean())


def _union_polygons(polygons: list[gdstk.Polygon]) -> list[gdstk.Polygon]:
    try:
        merged = gdstk.boolean(polygons, [], "or", precision=1e-3)
    except RuntimeError:
        return polygons
    if not merged:
        return polygons
    return list(merged)


def _extrude_polygon(xy: np.ndarray) -> pv.PolyData:
    if xy.ndim != 2:
        raise ValueError(tr("error.polygon_points_2d"))
    if xy.shape[1] != 2:
        raise ValueError(tr("error.polygon_points_shape"))

    if len(xy) >= 2 and np.allclose(xy[0], xy[-1]):
        xy = xy[:-1]
    if len(xy) < 3:
        raise ValueError(tr("error.polygon_vertices_min"))

    points3d = np.column_stack((xy, np.zeros(len(xy))))
    ring_end_indices = np.array([len(xy)], dtype=np.uint32)
    tri_idx = earcut.triangulate_float64(xy, ring_end_indices)
    if len(tri_idx) == 0:
        raise ValueError(tr("error.triangulation_failed"))

    faces = np.empty((len(tri_idx) // 3, 4), dtype=np.int64)
    faces[:, 0] = 3
    faces[:, 1:] = tri_idx.reshape(-1, 3)

    poly = pv.PolyData(points3d, faces.ravel()).clean()
    mesh = poly.extrude((0.0, 0.0, 1.0), capping=True).clean()
    return cast(pv.PolyData, mesh)
