import pyvista as pv
from pyvista import examples


mesh = examples.download_bunny_coarse()

pl = pv.Plotter()
pl.add_mesh(mesh, show_edges=True, color='white')
pl.add_points(mesh.points, color='red',
              point_size=2)
pl.camera_position = pv.CameraPosition(
    position=(0.02, 0.30, 0.73),
    focal_point=(0.02, 0.03, -0.022),
    viewup=(-0.03, 0.94, -0.34)
)
pl.show()
