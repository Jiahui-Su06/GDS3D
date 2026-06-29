from __future__ import annotations

from pathlib import Path
from typing import Any

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QColor, QIcon
from PySide6.QtWidgets import (
    QColorDialog,
    QDoubleSpinBox,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QScrollArea,
    QWidget,
)

from i18n import tr
from objects import BaseplateObject, Bounds2D, GdsLayerObject, SceneObject


RESET_ICON = QIcon(str(Path(__file__).resolve().parent / "icons" / "reset.svg"))


class PropertyPanel(QScrollArea):
    property_changed = Signal(str, str, object)
    reset_requested = Signal(str, str)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._object_id: str | None = None
        self._container = QWidget()
        self._layout = QFormLayout(self._container)
        self._layout.setFieldGrowthPolicy(
            QFormLayout.FieldGrowthPolicy.AllNonFixedFieldsGrow
        )
        self._layout.setLabelAlignment(Qt.AlignmentFlag.AlignRight)

        self.setWidgetResizable(True)
        self.setWidget(self._container)
        self.show_empty()

    def set_object(self, obj: SceneObject | None) -> None:
        self._clear()
        self._object_id = obj.id if obj is not None else None
        if obj is None:
            self.show_empty()
            return

        self._add_text(tr("property.name"), "name", obj.name)
        if isinstance(obj, GdsLayerObject):
            self._add_readonly(tr("property.layer"), str(obj.layer))
            self._add_readonly(tr("property.datatype"), str(obj.datatype))
        self._add_color(tr("property.color"), "color", obj.color)
        self._add_float(
            tr("property.brightness"), "brightness", obj.brightness, 0.0, 2.0, step=0.05
        )
        self._add_float(tr("property.opacity"), "opacity", obj.opacity, 0.0, 1.0, step=0.05)

        if isinstance(obj, GdsLayerObject):
            self._add_readonly(tr("property.file"), _short_path(obj.file_path))
            self._add_readonly(tr("property.cell"), obj.cell_name)
            self._add_bounds_readonly(obj)
            self._add_float(
                tr("property.z_min"), "z_min", obj.z_min, -1_000_000.0, 1_000_000.0
            )
            self._add_float(
                tr("property.z_max"), "z_max", obj.z_max, -1_000_000.0, 1_000_000.0
            )
        elif isinstance(obj, BaseplateObject):
            self._add_float(
                tr("property.x_min"), "min_x", obj.bounds.min_x, -1_000_000.0, 1_000_000.0
            )
            self._add_float(
                tr("property.x_max"), "max_x", obj.bounds.max_x, -1_000_000.0, 1_000_000.0
            )
            self._add_float(
                tr("property.y_min"), "min_y", obj.bounds.min_y, -1_000_000.0, 1_000_000.0
            )
            self._add_float(
                tr("property.y_max"), "max_y", obj.bounds.max_y, -1_000_000.0, 1_000_000.0
            )
            self._add_float(
                tr("property.z_min"), "z_min", obj.z_min, -1_000_000.0, 1_000_000.0
            )
            self._add_float(
                tr("property.z_max"), "z_max", obj.z_max, -1_000_000.0, 1_000_000.0
            )

    def show_scene_summary(self, object_count: int) -> None:
        self._clear()
        self._object_id = None
        self._add_readonly(tr("property.selection"), tr("property.selection_scene"))
        self._add_readonly(tr("property.objects"), str(object_count))

    def show_cell_summary(
        self,
        name: str,
        file_path: Path,
        layer_count: int,
        bounds: Bounds2D | None,
        z_min: float | None,
        z_max: float | None,
    ) -> None:
        self._clear()
        self._object_id = None
        self._add_readonly(tr("property.selection"), tr("property.selection_cell"))
        self._add_readonly(tr("property.cell"), name)
        self._add_readonly(tr("property.file"), _short_path(file_path))
        self._add_readonly(tr("property.layers"), str(layer_count))
        if bounds is not None:
            self._add_readonly(tr("property.x_min"), f"{bounds.min_x:.4f}")
            self._add_readonly(tr("property.x_max"), f"{bounds.max_x:.4f}")
            self._add_readonly(tr("property.y_min"), f"{bounds.min_y:.4f}")
            self._add_readonly(tr("property.y_max"), f"{bounds.max_y:.4f}")
        if z_min is not None and z_max is not None:
            self._add_readonly(tr("property.z_min"), f"{z_min:.4f}")
            self._add_readonly(tr("property.z_max"), f"{z_max:.4f}")

    def show_empty(self) -> None:
        self._clear()
        self._object_id = None
        label = QLabel(tr("property.no_component_selected"))
        label.setObjectName("emptyLabel")
        self._layout.addRow(label)

    def _emit(self, field: str, value: Any) -> None:
        if self._object_id is not None:
            self.property_changed.emit(self._object_id, field, value)

    def _emit_reset(self, field: str) -> None:
        if self._object_id is not None:
            self.reset_requested.emit(self._object_id, field)

    def _clear(self) -> None:
        while self._layout.count():
            item = self._layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()

    def _add_readonly(self, label: str, value: str) -> None:
        field = QLineEdit(value)
        field.setReadOnly(True)
        self._layout.addRow(label, field)

    def _add_text(self, label: str, field: str, value: str) -> None:
        editor = QLineEdit(value)
        editor.editingFinished.connect(lambda: self._emit(field, editor.text()))
        self._layout.addRow(label, self._with_reset(editor, field))

    def _add_float(
        self,
        label: str,
        field: str,
        value: float,
        minimum: float,
        maximum: float,
        step: float = 1.0,
    ) -> None:
        editor = QDoubleSpinBox()
        editor.setRange(minimum, maximum)
        editor.setDecimals(4)
        editor.setSingleStep(step)
        editor.setKeyboardTracking(False)
        editor.setButtonSymbols(QDoubleSpinBox.ButtonSymbols.UpDownArrows)
        editor.setFrame(True)
        editor.setValue(value)
        editor.valueChanged.connect(
            lambda new_value: self._emit(field, float(new_value))
        )
        self._layout.addRow(label, self._with_reset(editor, field))

    def _add_color(self, label: str, field: str, value: str) -> None:
        button = QPushButton(value)
        button.setProperty("colorValue", value)
        button.clicked.connect(lambda: self._choose_color(field, button))
        self._layout.addRow(label, self._with_reset(button, field))

    def _choose_color(self, field: str, button: QPushButton) -> None:
        current = QColor(button.text())
        color = QColorDialog.getColor(current, self, tr("dialog.select_color"))
        if not color.isValid():
            return
        value = color.name()
        button.setText(value)
        button.setProperty("colorValue", value)
        self._emit(field, value)

    def _add_bounds_readonly(self, obj: GdsLayerObject) -> None:
        self._add_readonly(tr("property.x_min"), f"{obj.bounds.min_x:.4f}")
        self._add_readonly(tr("property.x_max"), f"{obj.bounds.max_x:.4f}")
        self._add_readonly(tr("property.y_min"), f"{obj.bounds.min_y:.4f}")
        self._add_readonly(tr("property.y_max"), f"{obj.bounds.max_y:.4f}")

    def _with_reset(self, editor: QWidget, field: str) -> QWidget:
        wrapper = QWidget()
        layout = QHBoxLayout(wrapper)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(4)

        reset_button = QPushButton()
        reset_button.setObjectName("resetButton")
        reset_button.setIcon(RESET_ICON)
        reset_button.setToolTip(tr("property.reset"))
        reset_button.clicked.connect(lambda: self._emit_reset(field))

        layout.addWidget(editor, 1)
        layout.addWidget(reset_button)
        return wrapper


def _short_path(path: Path) -> str:
    return str(path)
