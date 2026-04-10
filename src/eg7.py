import pyvista as pv
from pyvista import examples
import numpy as np


mesh = examples.load_airplane()
print(mesh.n_cells)
print(mesh.n_points)
print(mesh.n_arrays)
print(mesh.bounds)
print(mesh.center)

the_pts = mesh.points
print(isinstance(the_pts, np.ndarray))

print(the_pts[0:5, :])
