import pyvista as pv
from pyvista import examples
import numpy as np


mesh = examples.load_hexbeam()
cpos = pv.CameraPosition(position=(6.20, 3.00, 7.50),
                         focal_point=(0.16, 0.13, 2.65),
                         viewup=(-0.28, 0.94, -0.21))

pl = pv.Plotter()
pl.add_mesh(mesh, show_edges=True, color='white')
pl.add_points(mesh.points, color='red',
              point_size=20)
pl.camera_position = cpos
pl.show()

mesh.point_data['my point values'] = np.arange(mesh.n_points)
mesh.plot(scalars='my point values', cpos=cpos, show_edges=True)

mesh.cell_data['my cell values'] = np.arange(mesh.n_cells)
mesh.plot(scalars='my cell values', cpos=cpos, show_edges=True)
