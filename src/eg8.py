import pyvista as pv
from pyvista import examples
import numpy as np


mesh = examples.load_uniform()
arr = mesh.point_data['Spatial Point Data']

mesh.cell_data['foo'] = np.random.rand(mesh.n_cells)
foo = mesh['foo']

print(isinstance(foo, np.ndarray))
mesh['new-array'] = np.random.rand(mesh.n_points)

# TODO: 这个是需要做的事情