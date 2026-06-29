from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Literal

from PySide6.QtWidgets import (
    QComboBox,
    QDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QVBoxLayout,
)

from i18n import tr


ExportFormat = Literal["png", "svg", "pdf", "gltf"]
ExportQuality = Literal["low", "standard", "high"]
ExportSizePreset = Literal["figure_4_3", "figure_3_2", "wide_16_9", "square_1_1"]

EXPORT_FORMATS: tuple[ExportFormat, ...] = ("png", "svg", "pdf", "gltf")
EXPORT_QUALITIES: tuple[ExportQuality, ...] = ("low", "standard", "high")
EXPORT_SIZE_PRESETS: tuple[ExportSizePreset, ...] = (
    "figure_4_3",
    "figure_3_2",
    "wide_16_9",
    "square_1_1",
)
EXPORT_QUALITY_WIDTHS: dict[ExportQuality, int] = {
    "low": 2000,
    "standard": 4000,
    "high": 6000,
}
EXPORT_SIZE_RATIOS: dict[ExportSizePreset, tuple[int, int]] = {
    "figure_4_3": (4, 3),
    "figure_3_2": (3, 2),
    "wide_16_9": (16, 9),
    "square_1_1": (1, 1),
}
EXPORT_PIXEL_COUNT_MAX = 36_000_000


@dataclass(frozen=True)
class ExportOptions:
    file_format: ExportFormat
    quality: ExportQuality
    size_preset: ExportSizePreset
    image_size: tuple[int, int] | None


class ExportDialog(QDialog):
    def __init__(
        self,
        default_format: ExportFormat,
        default_quality: ExportQuality,
        default_size_preset: ExportSizePreset,
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.setWindowTitle(tr("dialog.export_as"))
        self.setModal(True)
        self.setMinimumSize(330, 230)

        self._format_combo = QComboBox()
        self._format_combo.setObjectName("formatCombo")
        for file_format in EXPORT_FORMATS:
            self._format_combo.addItem(_format_label(file_format), file_format)
        format_index = self._format_combo.findData(default_format)
        self._format_combo.setCurrentIndex(max(format_index, 0))

        self._size_label = _section_label(tr("export.size"))
        self._size_combo = QComboBox()
        self._size_combo.setObjectName("sizeCombo")
        for size_preset in EXPORT_SIZE_PRESETS:
            self._size_combo.addItem(tr(f"export.size_{size_preset}"), size_preset)
        size_index = self._size_combo.findData(default_size_preset)
        self._size_combo.setCurrentIndex(max(size_index, 0))

        self._quality_label = _section_label(tr("export.quality"))
        self._quality_combo = QComboBox()
        self._quality_combo.setObjectName("qualityCombo")
        for quality in EXPORT_QUALITIES:
            self._quality_combo.addItem(tr(f"export.quality_{quality}"), quality)
        quality_index = self._quality_combo.findData(default_quality)
        self._quality_combo.setCurrentIndex(max(quality_index, 0))

        self._summary_label = QLabel()
        self._summary_label.setObjectName("exportSummary")

        ok_button = QPushButton("OK")
        cancel_button = QPushButton("Cancel")
        ok_button.setObjectName("dialogButton")
        cancel_button.setObjectName("dialogButton")
        ok_button.clicked.connect(self.accept)
        cancel_button.clicked.connect(self.reject)
        button_layout = QHBoxLayout()
        button_layout.setContentsMargins(0, 0, 0, 0)
        button_layout.setSpacing(8)
        button_layout.addStretch(1)
        button_layout.addWidget(ok_button)
        button_layout.addWidget(cancel_button)
        button_layout.addStretch(1)

        settings_layout = QVBoxLayout()
        settings_layout.setContentsMargins(20, 20, 20, 18)
        settings_layout.setSpacing(10)
        settings_layout.addWidget(_section_label(tr("export.format")))
        settings_layout.addWidget(self._format_combo)
        settings_layout.addSpacing(10)
        settings_layout.addWidget(self._size_label)
        settings_layout.addWidget(self._size_combo)
        settings_layout.addSpacing(10)
        settings_layout.addWidget(self._quality_label)
        settings_layout.addWidget(self._quality_combo)
        settings_layout.addWidget(self._summary_label)
        settings_layout.addStretch(1)
        settings_layout.addLayout(button_layout)

        settings_frame = QFrame()
        settings_frame.setObjectName("exportSettings")
        settings_frame.setLayout(settings_layout)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(18, 18, 18, 18)
        layout.setSpacing(10)
        layout.addWidget(settings_frame)

        self.setStyleSheet(
            """
            QDialog {
                background: #f6f7f9;
            }
            QFrame#exportSettings {
                min-width: 300px;
                background: #f6f7f9;
            }
            QLabel[role="section"] {
                color: #1f2328;
                font-weight: 600;
                background: transparent;
            }
            QLabel#exportSummary {
                min-height: 22px;
                padding: 4px 0 0 0;
                border: none;
                background: transparent;
                color: #4f5b67;
            }
            QComboBox#formatCombo,
            QComboBox#sizeCombo,
            QComboBox#qualityCombo {
                min-height: 30px;
                padding: 3px 28px 3px 8px;
                border: 1px solid #b8c2cc;
                border-radius: 2px;
                background: #ffffff;
                color: #1f2328;
            }
            QComboBox#formatCombo:hover,
            QComboBox#sizeCombo:hover,
            QComboBox#qualityCombo:hover {
                border-color: #6d8fbd;
                background: #f9fbfd;
            }
            QComboBox#formatCombo::drop-down,
            QComboBox#sizeCombo::drop-down,
            QComboBox#qualityCombo::drop-down {
                subcontrol-origin: padding;
                subcontrol-position: top right;
                width: 24px;
                border-left: 1px solid #9aa8b8;
                background: #dce5ef;
            }
            QComboBox#formatCombo::drop-down:hover,
            QComboBox#sizeCombo::drop-down:hover,
            QComboBox#qualityCombo::drop-down:hover {
                background: #9fbce3;
            }
            QComboBox#formatCombo::down-arrow,
            QComboBox#sizeCombo::down-arrow,
            QComboBox#qualityCombo::down-arrow {
                image: url("$SPIN_DOWN_ICON");
                width: 8px;
                height: 8px;
            }
            QPushButton#dialogButton {
                min-width: 78px;
                min-height: 28px;
                padding: 3px 10px;
                border: 1px solid #b8c2cc;
                border-radius: 2px;
                background: #ffffff;
                color: #1f2328;
            }
            QPushButton#dialogButton:hover {
                background: #b9d0ee;
                border-color: #6d8fbd;
            }
            """
            .replace("$SPIN_DOWN_ICON", _icon_path("spin_down.svg"))
        )
        self._format_combo.currentIndexChanged.connect(self._sync_quality_visibility)
        self._size_combo.currentIndexChanged.connect(self._update_summary)
        self._quality_combo.currentIndexChanged.connect(self._update_summary)
        self._sync_quality_visibility()

    def options(self) -> ExportOptions:
        file_format = self._current_format()
        quality = self._quality_combo.currentData()
        if quality not in EXPORT_QUALITIES:
            quality = "standard"
        size_preset = self._size_combo.currentData()
        if size_preset not in EXPORT_SIZE_PRESETS:
            size_preset = "figure_4_3"
        image_size = None
        if file_format in {"png", "svg", "pdf"}:
            image_size = export_image_size(size_preset, quality)
        return ExportOptions(
            file_format=file_format,
            quality=quality,
            size_preset=size_preset,
            image_size=image_size,
        )

    def _current_format(self) -> ExportFormat:
        value = self._format_combo.currentData()
        if value in EXPORT_FORMATS:
            return value
        return "png"

    def _sync_quality_visibility(self) -> None:
        file_format = self._current_format()
        has_image_options = file_format in {"png", "svg", "pdf"}
        self._size_label.setVisible(has_image_options)
        self._size_combo.setVisible(has_image_options)
        self._quality_label.setVisible(has_image_options)
        self._quality_combo.setVisible(has_image_options)
        self._summary_label.setVisible(has_image_options)
        self._update_summary()

    def _update_summary(self) -> None:
        options = self.options()
        if options.image_size is None:
            self._summary_label.clear()
            return
        width, height = options.image_size
        ratio_width, ratio_height = EXPORT_SIZE_RATIOS[options.size_preset]
        self._summary_label.setText(
            tr(
                "export.output_summary",
                width=width,
                height=height,
                ratio=f"{ratio_width}:{ratio_height}",
            )
        )


def _section_label(text: str) -> QLabel:
    label = QLabel(text)
    label.setProperty("role", "section")
    return label


def _format_label(file_format: ExportFormat) -> str:
    if file_format == "png":
        return "PNG"
    if file_format == "svg":
        return "SVG"
    if file_format == "gltf":
        return "glTF"
    return "PDF"


def export_image_size(
    size_preset: ExportSizePreset,
    quality: ExportQuality,
) -> tuple[int, int]:
    ratio_width, ratio_height = EXPORT_SIZE_RATIOS[size_preset]
    width = EXPORT_QUALITY_WIDTHS[quality]
    height = round(width * ratio_height / ratio_width)
    size = (width, height)
    return _safe_image_size(size)


def _safe_image_size(size: tuple[int, int]) -> tuple[int, int]:
    width, height = size
    pixel_count = width * height
    if pixel_count <= EXPORT_PIXEL_COUNT_MAX:
        return size

    ratio = (EXPORT_PIXEL_COUNT_MAX / pixel_count) ** 0.5
    safe_width = max(1, int(width * ratio))
    safe_height = max(1, int(height * ratio))
    return (safe_width, safe_height)


def _icon_path(name: str) -> str:
    return (Path(__file__).resolve().parent / "icons" / name).as_posix()
