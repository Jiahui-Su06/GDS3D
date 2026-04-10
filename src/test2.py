import pyvista as pv

def make_tapered_fiber(tip_point=(0, 0, 0), r=0.5, L_cone=2.0, L_cyl=4.0, resolution=80):
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


fiber, cone, cyl = make_tapered_fiber(
    tip_point=(1, 2, 0),   # 圆锥尖端放在这里
    r=50,
    L_cone=50,
    L_cyl=200,
    resolution=100,
)

pl = pv.Plotter()
pl.add_mesh(cone, color="deepskyblue", smooth_shading=True)
pl.add_mesh(cyl, color="deepskyblue", smooth_shading=True)
pl.add_mesh(pv.Sphere(radius=0.08, center=(1, 2, 0)), color="red")  # 标出 tip_point
pl.add_axes()
pl.show()