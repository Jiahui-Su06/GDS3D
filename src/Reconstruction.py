from __future__ import annotations

from pathlib import Path
import numpy as np
import gdstk
import pyvista as pv
import mapbox_earcut as earcut


# 读取GDS文件
GDS_FILE = Path(r"E:\reconstruction.gds")

# 选择GDS文件需要读取的层，layer=(4, 1)
LAYER = 1
DATATYPE = 0

# z 方向
z0 = 0.0
z1 = 15.0

# ========= 2) GDS polygon -> 先三角化再挤出 =========
def polygon_to_extruded_mesh(
    xy: np.ndarray,
    z0: float,
    z1: float
) -> pv.PolyData:
    xy = np.asarray(xy, dtype=np.float64)

    # 去掉首尾重复点，并做多边形点数检查
    if len(xy) >= 2 and np.allclose(xy[0], xy[-1]):
        xy = xy[:-1]
    if len(xy) < 3:
        raise ValueError("polygon needs at least 3 vertices")

    # 2D -> 3D 底面点
    points2d = xy
    points3d = np.c_[points2d, np.full(len(points2d), z0)]

    # earcut 需要 ring_end_indices
    ring_end_indices = np.array([len(points2d)], dtype=np.uint32)

    # 三角化，返回顶点索引序列，例如 [0,1,2, 0,2,3, ...]
    tri_idx = earcut.triangulate_float64(points2d, ring_end_indices)

    if len(tri_idx) == 0:
        raise ValueError("triangulation failed")

    # PyVista faces: 每个三角形写成 [3, i, j, k]
    tri_idx = tri_idx.reshape(-1, 3)
    faces = np.hstack(
        [np.array([3, a, b, c], dtype=np.int64) for a, b, c in tri_idx]
    )

    poly = pv.PolyData(points3d, faces=faces)

    # 先 clean 一下更稳
    poly = poly.clean()

    mesh = poly.extrude((0, 0, z1 - z0), capping=True)
    return mesh.clean()


# 读取GDS文件，做检查
if not GDS_FILE.exists():
    raise FileNotFoundError(f"GDS file not found: {GDS_FILE}")

lib = gdstk.read_gds(str(GDS_FILE))
top_cells = lib.top_level()
if not top_cells:
    raise RuntimeError("No top-level cell found in GDS")

top = None
for cell in top_cells:
    if cell.name == "Unnamed_0":
        top = cell
        break

if top is None:
    raise RuntimeError("Cell 'AWG' not found")

polys = top.get_polygons(
    apply_repetitions=True,
    include_paths=True,
    depth=None,
    layer=LAYER,
    datatype=DATATYPE,
)

print(f"Found {len(polys)} polygons on layer/datatype = ({LAYER}, {DATATYPE})")


# 转为3D
meshes = []
for i, poly in enumerate(polys):
    xy = np.asarray(poly.points if hasattr(poly, "points") else poly)

    try:
        mesh = polygon_to_extruded_mesh(xy, z0, z1)
        meshes.append(mesh)
    except Exception as e:
        print(f"skip polygon {i}: {e}")

if not meshes:
    raise RuntimeError("No valid polygons converted to 3D meshes.")

print(f"Valid meshes: {len(meshes)}")


############################################################ 
# 绘图模块
############################################################
pl = pv.Plotter()

# 颜色定义
colors = {
    "si_substrate": "#343399",
    "sio2_box": "#B6CAFF",
    "sin_core": "#0000FE",
    "sio2_clad": "#B6CAFF",
}
opacity = {
    "si_substrate": 1.0,
    "sio2_box": 1.0,
    "sin_core": 1.0,
    "sio2_clad": 0.22,
}

# 绘制AWG器件主体部分（氮化硅层）
for mesh in meshes:
    pl.add_mesh(
        mesh,
        color=colors["sin_core"],
        opacity=opacity["sin_core"],
        show_edges=False,
        # smooth_shading=True,
        # ambient=0.85,
        # diffuse=0.2,
        # specular=0.0,
    )

# sin_core_mesh = meshes[0]

# for m in meshes[1:]:
#     sin_core_mesh = sin_core_mesh.merge(m)

# sin_core_mesh = sin_core_mesh.clean()
# sin_core_mesh = sin_core_mesh.compute_normals(
#     cell_normals=False,
#     point_normals=True,
#     split_vertices=False,
#     auto_orient_normals=True,
# )

# pl.add_mesh(
#     sin_core_mesh,
#     color=colors["sin_core"],
#     opacity=opacity["sin_core"],
#     show_edges=False,
#     smooth_shading=True,
#     specular=0.08,
# )

xmin = -250
xmax = 1450
ymin = -400
ymax = 400

# 绘制Si层
substrate_Si = pv.Box(bounds=(xmin, xmax, ymin, ymax, -50, -20))
pl.add_mesh(substrate_Si, color=colors["si_substrate"], show_edges=False)

# 绘制BOX层
BOX_SiO2 = pv.Box(bounds=(xmin, xmax, ymin, ymax, -20, 0))
pl.add_mesh(BOX_SiO2, color=colors["sio2_box"], show_edges=False)

# 绘制Clad
clad_SiO2 = pv.Box(bounds=(xmin, xmax, ymin, ymax, 0, 20))
pl.add_mesh(clad_SiO2, color=colors["sio2_box"], opacity=opacity["sio2_clad"], show_edges=False)

# light = pv.Light(
#     position=(-2000, 2000, 200),
#     focal_point=(0, 0, 0),
#     color="white",
#     intensity=0.7,
#     positional=True,
# )
# pl.add_light(light)

# pl.add_axes()
# pl.show_grid()
pl.show()




# pl.add_mesh(si_substrate_mesh, color=colors["si_substrate"], opacity=opacity["si_substrate"])
# pl.add_mesh(box_mesh,          color=colors["sio2_box"],    opacity=opacity["sio2_box"])
# pl.add_mesh(sin_mesh,          color=colors["sin_core"],    opacity=opacity["sin_core"])
# pl.add_mesh(clad_mesh,         color=colors["sio2_clad"],   opacity=opacity["sio2_clad"])

# pl.set_background("white")
# pl.show()