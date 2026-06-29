from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import gdstk
import numpy as np

from i18n import tr
from objects import Bounds2D


@dataclass(frozen=True)
class GdsLayerData:
    file_path: Path
    cell_name: str
    layer: int
    datatype: int
    bounds: Bounds2D
    polygons: list[gdstk.Polygon]


@dataclass(frozen=True)
class GdsLayerSelection:
    cell_name: str
    layer: int
    datatype: int


@dataclass(frozen=True)
class GdsLayerInfo:
    selection: GdsLayerSelection
    polygon_count: int
    bounds: Bounds2D


@dataclass(frozen=True)
class GdsCellInfo:
    name: str
    layers: list[GdsLayerInfo]


@dataclass(frozen=True)
class GdsFileInfo:
    file_path: Path
    cells: list[GdsCellInfo]


def load_default_gds_layer(file_path: Path) -> GdsLayerData:
    """Load the first useful GDS layer using conservative defaults."""
    path = file_path.expanduser().resolve()
    if not path.exists():
        raise FileNotFoundError(path)
    if path.suffix.lower() != ".gds":
        raise ValueError(tr("error.selected_not_gds"))

    lib = gdstk.read_gds(str(path))
    top_cells = [cell for cell in lib.top_level() if isinstance(cell, gdstk.Cell)]
    if not top_cells:
        raise ValueError(tr("error.gds_no_top_cell"))

    cell = _choose_cell(top_cells)
    all_polygons = cell.get_polygons(
        apply_repetitions=True, include_paths=True, depth=None
    )
    if not all_polygons:
        raise ValueError(tr("error.gds_cell_no_polygons", name=cell.name))

    layer, datatype = _choose_layer_pair(all_polygons)
    polygons = [
        poly
        for poly in all_polygons
        if int(poly.layer) == layer and int(poly.datatype) == datatype
    ]
    if not polygons:
        raise ValueError(
            tr("error.gds_layer_no_polygons", layer=layer, datatype=datatype)
        )

    return GdsLayerData(
        file_path=path,
        cell_name=cell.name,
        layer=layer,
        datatype=datatype,
        bounds=_compute_bounds(polygons),
        polygons=polygons,
    )


def inspect_gds_file(file_path: Path) -> GdsFileInfo:
    path = _resolve_gds_path(file_path)
    lib = gdstk.read_gds(str(path))
    cells = _display_cells(lib)
    if not cells:
        raise ValueError(tr("error.gds_no_cell"))

    cell_infos: list[GdsCellInfo] = []
    for cell in cells:
        polygons = _cell_polygons(cell)
        layer_infos: list[GdsLayerInfo] = []
        for (layer, datatype), layer_polygons in _polygons_by_layer(polygons).items():
            bounds = _try_compute_bounds(layer_polygons)
            if bounds is None:
                continue
            layer_infos.append(
                GdsLayerInfo(
                    selection=GdsLayerSelection(cell.name, layer, datatype),
                    polygon_count=len(layer_polygons),
                    bounds=bounds,
                )
            )
        if layer_infos:
            cell_infos.append(GdsCellInfo(cell.name, layer_infos))

    if not cell_infos:
        raise ValueError(tr("error.gds_no_renderable_layers"))

    return GdsFileInfo(file_path=path, cells=cell_infos)


def load_gds_layers(
    file_path: Path, selections: list[GdsLayerSelection]
) -> list[GdsLayerData]:
    path = _resolve_gds_path(file_path)
    if not selections:
        return []

    lib = gdstk.read_gds(str(path))
    cells = {cell.name: cell for cell in lib.cells if isinstance(cell, gdstk.Cell)}

    data: list[GdsLayerData] = []
    for selection in selections:
        cell = cells.get(selection.cell_name)
        if cell is None:
            raise ValueError(tr("error.gds_cell_missing", name=selection.cell_name))

        polygons = [
            poly
            for poly in _cell_polygons(cell)
            if int(poly.layer) == selection.layer
            and int(poly.datatype) == selection.datatype
        ]
        bounds = _try_compute_bounds(polygons)
        if bounds is None:
            raise ValueError(
                tr(
                    "error.gds_no_renderable_polygons",
                    cell_name=selection.cell_name,
                    layer=selection.layer,
                    datatype=selection.datatype,
                )
            )

        data.append(
            GdsLayerData(
                file_path=path,
                cell_name=selection.cell_name,
                layer=selection.layer,
                datatype=selection.datatype,
                bounds=bounds,
                polygons=polygons,
            )
        )

    return data


def _choose_cell(cells: list[gdstk.Cell]) -> gdstk.Cell:
    for cell in cells:
        if cell.name == "AWG":
            return cell
    return cells[0]


def _display_cells(lib: gdstk.Library) -> list[gdstk.Cell]:
    top_cells = [
        cell
        for cell in lib.top_level()
        if isinstance(cell, gdstk.Cell) and not _is_metadata_cell(cell.name)
    ]
    if top_cells:
        return sorted(top_cells, key=lambda cell: cell.name.casefold())

    cells = [
        cell
        for cell in lib.cells
        if isinstance(cell, gdstk.Cell) and not _is_metadata_cell(cell.name)
    ]
    return sorted(cells, key=lambda cell: cell.name.casefold())


def _is_metadata_cell(name: str) -> bool:
    return name.startswith("$$$") and name.endswith("$$$")


def _choose_layer_pair(polygons: list[gdstk.Polygon]) -> tuple[int, int]:
    pairs = sorted({(int(poly.layer), int(poly.datatype)) for poly in polygons})
    if (4, 1) in pairs:
        return (4, 1)
    return pairs[0]


def _resolve_gds_path(file_path: Path) -> Path:
    path = file_path.expanduser().resolve()
    if not path.exists():
        raise FileNotFoundError(path)
    if path.suffix.lower() != ".gds":
        raise ValueError(tr("error.selected_not_gds"))
    return path


def _cell_polygons(cell: gdstk.Cell) -> list[gdstk.Polygon]:
    return cell.get_polygons(apply_repetitions=True, include_paths=True, depth=None)


def _polygons_by_layer(
    polygons: list[gdstk.Polygon],
) -> dict[tuple[int, int], list[gdstk.Polygon]]:
    groups: dict[tuple[int, int], list[gdstk.Polygon]] = {}
    for poly in polygons:
        key = (int(poly.layer), int(poly.datatype))
        groups.setdefault(key, []).append(poly)
    return dict(sorted(groups.items()))


def _compute_bounds(polygons: list[gdstk.Polygon]) -> Bounds2D:
    bounds = _try_compute_bounds(polygons)
    if bounds is None:
        raise ValueError(tr("error.bounds_degenerate"))
    return bounds


def _try_compute_bounds(polygons: list[gdstk.Polygon]) -> Bounds2D | None:
    points = [np.asarray(poly.points, dtype=np.float64) for poly in polygons]
    if not points:
        return None

    xy = np.vstack(points)
    if xy.shape[1] != 2:
        raise ValueError(tr("error.gds_points_2d"))

    min_x = float(np.min(xy[:, 0]))
    min_y = float(np.min(xy[:, 1]))
    max_x = float(np.max(xy[:, 0]))
    max_y = float(np.max(xy[:, 1]))
    if min_x >= max_x or min_y >= max_y:
        return None
    return Bounds2D(min_x=min_x, min_y=min_y, max_x=max_x, max_y=max_y)
