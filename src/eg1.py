import pyvista as pv
import numpy as np


pv.set_plot_theme('document')
# pv.set_jupyter_backend('static')
pv.global_theme.window_size = [600, 400] # pixel
pv.global_theme.axes.show = False
pv.global_theme.anti_aliasing = 'fxaa'
pv.global_theme.show_scalar_bar = False


rng = np.random.default_rng(seed=0)
points = rng.random((100, 3))
mesh = pv.PolyData(points)
mesh.plot(point_size=10, style='points')
