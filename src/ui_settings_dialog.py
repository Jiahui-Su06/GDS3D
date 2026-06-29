from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from PySide6.QtGui import QIcon
from PySide6.QtWidgets import (
    QCheckBox,
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QHBoxLayout,
    QPushButton,
    QSpinBox,
    QWidget,
)

from i18n import tr


LEFT_PANEL_MIN_WIDTH_DEFAULT = 220
RIGHT_PANEL_MIN_WIDTH_DEFAULT = 360
RESET_ICON = QIcon(str(Path(__file__).resolve().parent / "icons" / "reset.svg"))


@dataclass(frozen=True)
class UiSettings:
    left_panel_min_width: int
    right_panel_min_width: int
    show_axes: bool


class UiSettingsDialog(QDialog):
    def __init__(self, settings: UiSettings, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle(tr("settings.title"))
        self.setModal(True)

        self._left_width = _make_width_editor(settings.left_panel_min_width)
        self._right_width = _make_width_editor(settings.right_panel_min_width)
        self._show_axes = QCheckBox()
        self._show_axes.setChecked(settings.show_axes)

        buttons = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)

        layout = QFormLayout(self)
        layout.setFieldGrowthPolicy(QFormLayout.FieldGrowthPolicy.AllNonFixedFieldsGrow)
        layout.addRow(
            tr("settings.components_min_width"),
            _with_reset(
                self._left_width,
                lambda: self._left_width.setValue(LEFT_PANEL_MIN_WIDTH_DEFAULT),
            ),
        )
        layout.addRow(
            tr("settings.properties_min_width"),
            _with_reset(
                self._right_width,
                lambda: self._right_width.setValue(RIGHT_PANEL_MIN_WIDTH_DEFAULT),
            ),
        )
        layout.addRow(tr("settings.show_xyz_axes"), self._show_axes)
        layout.addRow(buttons)

    def settings(self) -> UiSettings:
        return UiSettings(
            left_panel_min_width=self._left_width.value(),
            right_panel_min_width=self._right_width.value(),
            show_axes=self._show_axes.isChecked(),
        )


def _make_width_editor(value: int) -> QSpinBox:
    editor = QSpinBox()
    editor.setRange(120, 800)
    editor.setSingleStep(10)
    editor.setSuffix(" px")
    editor.setValue(value)
    return editor


def _with_reset(editor: QWidget, reset: Callable[[], None]) -> QWidget:
    wrapper = QWidget()
    layout = QHBoxLayout(wrapper)
    layout.setContentsMargins(0, 0, 0, 0)
    layout.setSpacing(6)

    reset_button = QPushButton()
    reset_button.setObjectName("resetButton")
    reset_button.setIcon(RESET_ICON)
    reset_button.setToolTip(tr("settings.reset_to_default"))
    reset_button.clicked.connect(reset)

    layout.addWidget(editor, 1)
    layout.addWidget(reset_button)
    return wrapper
