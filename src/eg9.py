import pyvista as pv
from pyvista import examples

mesh = examples.load_airplane()


pl = pv.Plotter()
pl.add_mesh(mesh=mesh)
pl.camera.zoom(2)
pl.show()

cpos = pl.camera_position
pl = pv.Plotter(off_screen=True)
pl.add_mesh(mesh, color='lightblue')
pl.camera_position = cpos
pl.show(screenshot='airplane.png')
