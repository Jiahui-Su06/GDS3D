from __future__ import annotations

from pathlib import Path
import numpy as np
import gdstk
import pyvista as pv
import mapbox_earcut as earcut


# 读取 GDS 文件
GDS_FILE = Path(r"E:\VSCode\Draw3D\docs\AWG_3.2nm_1.gds")

# 选择 GDS 文件需要读取的层
LAYER = 4
DATATYPE = 1

# z 方向（单位尽量和 GDS 一致；通常 GDS 是 um）
# 这里按常见 SiN 平台先给一个更合理的示意厚度
z0 = 0.0
z1 = 15


# ========= 1) 单个 polygon -> 先三角化再挤出 =========
def polygon_to_extruded_mesh(
    xy: np.ndarray,
    z0: float,
    z1: float
) -> pv.PolyData:
    xy = np.asarray(xy, dtype=np.float64)

    # 去掉首尾重复点
    if len(xy) >= 2 and np.allclose(xy[0], xy[-1]):
        xy = xy[:-1]

    if len(xy) < 3:
        raise ValueError("polygon needs at least 3 vertices")

    points2d = xy
    points3d = np.c_[points2d, np.full(len(points2d), z0)]

    # earcut 需要 ring_end_indices
    ring_end_indices = np.array([len(points2d)], dtype=np.uint32)

    # 三角化
    tri_idx = earcut.triangulate_float64(points2d, ring_end_indices)
    if len(tri_idx) == 0:
        raise ValueError("triangulation failed")

    tri_idx = tri_idx.reshape(-1, 3)

    # PyVista faces: [3, i, j, k, 3, i, j, k, ...]
    faces = np.hstack(
        [np.array([3, a, b, c], dtype=np.int64) for a, b, c in tri_idx]
    )

    poly = pv.PolyData(points3d, faces=faces).clean()
    mesh = poly.extrude((0, 0, z1 - z0), capping=True).clean()
    return mesh


# ========= 2) 读取 GDS =========
if not GDS_FILE.exists():
    raise FileNotFoundError(f"GDS file not found: {GDS_FILE}")

lib = gdstk.read_gds(str(GDS_FILE))
top_cells = lib.top_level()
if not top_cells:
    raise RuntimeError("No top-level cell found in GDS")

top = None
for cell in top_cells:
    if cell.name == "AWG":
        top = cell
        break

if top is None:
    raise RuntimeError("Cell 'AWG' not found")


# 原始 polygon
polys = top.get_polygons(
    apply_repetitions=True,
    include_paths=True,
    depth=None,
    layer=LAYER,
    datatype=DATATYPE,
)

print(f"Found {len(polys)} raw polygons on layer/datatype = ({LAYER}, {DATATYPE})")

if len(polys) == 0:
    raise RuntimeError(f"No polygons found on ({LAYER}, {DATATYPE})")


# ========= 3) 先做 2D 并集，减少拼块边界 =========
# 这一步是去除“分割线感”的关键
union_polys = gdstk.boolean(
    polys,
    [],
    "or",
    precision=1e-3,
)

print(f"After union: {len(union_polys)} polygons")


# ========= 4) 转成 3D =========
meshes = []
all_xy = []

for i, poly in enumerate(union_polys):
    xy = np.asarray(poly.points if hasattr(poly, "points") else poly)
    all_xy.append(xy)

    try:
        mesh = polygon_to_extruded_mesh(xy, z0, z1)
        meshes.append(mesh)
    except Exception as e:
        print(f"skip polygon {i}: {e}")

if not meshes:
    raise RuntimeError("No valid polygons converted to 3D meshes.")

print(f"Valid meshes: {len(meshes)}")


# 把 SiN 层合成一个显示 mesh
sin_mesh = meshes[0]
for m in meshes[1:]:
    sin_mesh = sin_mesh.merge(m)

sin_mesh = sin_mesh.clean()


# # ========= 5) 根据器件范围自动生成基底/包层大小 =========
# all_xy_concat = np.vstack(all_xy)
# xmin, ymin = all_xy_concat.min(axis=0)
# xmax, ymax = all_xy_concat.max(axis=0)

# pad = 30.0
xmin = -200
xmax = 610
ymin = -500
ymax = 900


# 层厚（示意图参数，单位和 GDS 一致）
si_bottom = -150.0
si_top = -50.0

box_bottom = -50.0
box_top = 0.0

clad_bottom = 0.0
clad_top = 20


# ========= 6) 绘图模块 =========
pl = pv.Plotter()

# 颜色定义（论文风格）
colors = {
    "si_substrate": "#343399",
    "sio2_box": "#B6CAFF",
    "sin_core": "#0000FE",
    "sio2_clad": "#F4F7FB",
}
opacity = {
    "si_substrate": 1.0,
    "sio2_box": 1.0,
    "sin_core": 1.0,
    "sio2_clad": 0.20,
}

# 绘制 Si 层
substrate_Si = pv.Box(bounds=(xmin, xmax, ymin, ymax, si_bottom, si_top))
pl.add_mesh(
    substrate_Si,
    color=colors["si_substrate"],
    opacity=opacity["si_substrate"],
    show_edges=False,
)

# 绘制 BOX 层
BOX_SiO2 = pv.Box(bounds=(xmin, xmax, ymin, ymax, box_bottom, box_top))
pl.add_mesh(
    BOX_SiO2,
    color=colors["sio2_box"],
    opacity=opacity["sio2_box"],
    show_edges=False,
)

# 绘制 AWG 器件主体（氮化硅层）
pl.add_mesh(
    sin_mesh,
    color=colors["sin_core"],
    opacity=opacity["sin_core"],
    show_edges=False,
    smooth_shading=True,
    specular=0.03,
)

# 绘制覆盖层 SiO2
clad_SiO2 = pv.Box(bounds=(xmin, xmax, ymin, ymax, clad_bottom, clad_top))
pl.add_mesh(
    clad_SiO2,
    color=colors["sio2_clad"],
    opacity=opacity["sio2_clad"],
    show_edges=False,
)

def make_tapered_fiber_xbackward(tip_point=(0, 0, 0), r=0.5, L_cone=2.0, L_cyl=4.0, resolution=80):
    """
    生成一个沿 x 负方向的锥形光纤（圆锥 + 圆柱），并以圆锥尖端为定位点。

    参数
    ----
    tip_point : tuple
        圆锥尖端的位置 (x, y, z)
    r : float
        圆柱半径 / 圆锥底面半径
    L_cone : float
        圆锥长度
    L_cyl : float
        圆柱长度
    resolution : int
        圆周离散精度

    返回
    ----
    fiber : pyvista.PolyData
        拼接后的光纤
    cone : pyvista.PolyData
        圆锥
    cyl : pyvista.PolyData
        圆柱
    """
    x0, y0, z0 = tip_point

    # 圆锥朝向 x 负方向
    # tip 在 x0
    # 底面中心在 x0 + L_cone
    # 整个圆锥中心在两者中点：x0 + L_cone/2
    cone = pv.Cone(
        center=(x0 + L_cone / 2, y0, z0),
        direction=(-1, 0, 0),
        height=L_cone,
        radius=r,
        resolution=resolution,
        capping=True,
    )

    # 圆柱接在圆锥底面后面
    # 圆柱范围: [x0 + L_cone, x0 + L_cone + L_cyl]
    # 所以圆柱中心:
    cyl = pv.Cylinder(
        center=(x0 + L_cone + L_cyl / 2, y0, z0),
        direction=(1, 0, 0),
        radius=r,
        height=L_cyl,
        resolution=resolution,
        capping=True,
    )

    fiber = cone.merge(cyl)
    return fiber, cone, cyl

def make_tapered_fiber_xforward(tip_point=(0, 0, 0), r=0.5, L_cone=2.0, L_cyl=4.0, resolution=80):
    """
    生成一个沿 x 正方向的锥形光纤（圆锥 + 圆柱），并以圆锥尖端为定位点。

    参数
    ----
    tip_point : tuple
        圆锥尖端的位置 (x, y, z)
    r : float
        圆柱半径 / 圆锥底面半径
    L_cone : float
        圆锥长度
    L_cyl : float
        圆柱长度
    resolution : int
        圆周离散精度

    返回
    ----
    fiber : pyvista.PolyData
        拼接后的光纤
    cone : pyvista.PolyData
        圆锥
    cyl : pyvista.PolyData
        圆柱
    """
    x0, y0, z0 = tip_point

    # 圆锥朝向 x 正方向
    # tip 在 x0
    # 底面中心在 x0 - L_cone
    # 圆锥中心在两者中点：x0 - L_cone/2
    cone = pv.Cone(
        center=(x0 - L_cone / 2, y0, z0),
        direction=(1, 0, 0),
        height=L_cone,
        radius=r,
        resolution=resolution,
        capping=True,
    )

    # 圆柱接在圆锥底面后面
    # 圆柱范围: [x0 - L_cone - L_cyl, x0 - L_cone]
    # 所以圆柱中心:
    cyl = pv.Cylinder(
        center=(x0 - L_cone - L_cyl / 2, y0, z0),
        direction=(1, 0, 0),
        radius=r,
        height=L_cyl,
        resolution=resolution,
        capping=True,
    )

    fiber = cone.merge(cyl)
    return fiber, cone, cyl


# fiber, cone, cyl = make_tapered_fiber_xbackward(
#     tip_point=(610, -130, z1/2),   # 圆锥尖端放在这里
#     r=15,
#     L_cone=15,
#     L_cyl=1000,
#     resolution=100,
# )
# pl.add_mesh(cone, color="#B6CAFF", smooth_shading=True)
# pl.add_mesh(cyl, color="#B6CAFF", smooth_shading=True)

# for i in range(0, 8):
#     fiber, cone, cyl = make_tapered_fiber_xforward(
#         tip_point=(-200, -200-i*35, z1/2),   # 圆锥尖端放在这里
#         r=15,
#         L_cone=15,
#         L_cyl=1000,
#         resolution=100,
#     )
#     pl.add_mesh(cone, color="#B6CAFF", smooth_shading=True)
#     pl.add_mesh(cyl, color="#B6CAFF", smooth_shading=True)

# pl.add_axes()
# pl.show_grid()
pl.set_background("white")
pl.show()
