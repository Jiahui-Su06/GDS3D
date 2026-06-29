from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

from PySide6.QtWidgets import (
    QButtonGroup,
    QDialog,
    QDialogButtonBox,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from i18n import tr


ExportFormat = Literal["png", "svg", "pdf", "gltf"]

EXPORT_FORMATS: tuple[ExportFormat, ...] = ("png", "svg", "pdf", "gltf")


@dataclass(frozen=True)
class ExportOptions:
    file_format: ExportFormat


class ExportDialog(QDialog):
    def __init__(
        self,
        default_format: ExportFormat,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.setWindowTitle(tr("dialog.export_as"))
        self.setModal(True)

        self._format_group = QButtonGroup(self)
        self._format_group.setExclusive(True)
        format_layout = QHBoxLayout()
        format_layout.setSpacing(10)
        format_layout.setContentsMargins(0, 0, 0, 0)
        for file_format in EXPORT_FORMATS:
            button = QPushButton()
            button.setText(_format_label(file_format))
            button.setCheckable(True)
            button.setMinimumWidth(104)
            button.setMinimumHeight(38)
            self._format_group.addButton(button)
            button.setProperty("fileFormat", file_format)
            format_layout.addWidget(button)
            if file_format == default_format:
                button.setChecked(True)

        if self._format_group.checkedButton() is None:
            first_button = self._format_group.buttons()[0]
            first_button.setChecked(True)

        buttons = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(12)
        layout.addWidget(QLabel(tr("export.format")))
        layout.addLayout(format_layout)
        layout.addWidget(buttons)
        layout.setStretch(1, 1)
        self.setStyleSheet(
            """
            QPushButton {
                padding: 8px 16px;
                border-radius: 8px;
                border: 1px solid #c5ccd4;
                background: #ffffff;
                color: #1f2328;
            }
            QPushButton:hover {
                background: #f3f6fa;
            }
            QPushButton:checked {
                background: #2d6cdf;
                color: #ffffff;
                border-color: #2d6cdf;
            }
            QPushButton:checked:hover {
                background: #255ac4;
            }
            """
        )

    def options(self) -> ExportOptions:
        button = self._format_group.checkedButton()
        file_format = button.property("fileFormat") if button is not None else None
        if file_format not in EXPORT_FORMATS:
            file_format = "png"
        return ExportOptions(file_format=file_format)


def _format_label(file_format: ExportFormat) -> str:
    if file_format == "png":
        return "PNG"
    if file_format == "svg":
        return "SVG"
    if file_format == "pdf":
        return "PDF"
    return "glTF"
