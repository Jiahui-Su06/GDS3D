from __future__ import annotations

from PySide6.QtCore import QSize, Qt
from PySide6.QtWidgets import (
    QAbstractItemView,
    QDialog,
    QDialogButtonBox,
    QHeaderView,
    QLabel,
    QMessageBox,
    QTreeWidget,
    QTreeWidgetItem,
    QVBoxLayout,
    QWidget,
)

from gds_loader import GdsFileInfo, GdsLayerSelection
from i18n import tr


SELECTION_ROLE = Qt.ItemDataRole.UserRole


class GdsImportDialog(QDialog):
    def __init__(self, info: GdsFileInfo, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle(tr("dialog.import_gds"))
        self.setModal(True)
        self.resize(760, 520)

        self._tree = QTreeWidget()
        self._tree.setColumnCount(3)
        self._tree.setHeaderLabels(
            [
                tr("gds_import.cell_layer"),
                tr("gds_import.polygons"),
                tr("gds_import.bounds"),
            ]
        )
        self._tree.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self._tree.itemChanged.connect(self._sync_child_checks)

        self._build_tree(info)
        self._check_only_layer_if_unambiguous()
        self._tree.expandToDepth(1)
        self._tree.setColumnWidth(0, 360)
        self._tree.setColumnWidth(1, 90)
        self._tree.header().setSectionResizeMode(2, QHeaderView.ResizeMode.Stretch)

        buttons = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        buttons.accepted.connect(self._accept_if_selected)
        buttons.rejected.connect(self.reject)

        layout = QVBoxLayout(self)
        layout.addWidget(QLabel(info.file_path.name))
        layout.addWidget(self._tree, 1)
        layout.addWidget(buttons)

    def selected_layers(self) -> list[GdsLayerSelection]:
        selections: list[GdsLayerSelection] = []
        for top_index in range(self._tree.topLevelItemCount()):
            cell_item = self._tree.topLevelItem(top_index)
            for layer_index in range(cell_item.childCount()):
                layer_item = cell_item.child(layer_index)
                if layer_item.checkState(0) != Qt.CheckState.Checked:
                    continue
                selection = layer_item.data(0, SELECTION_ROLE)
                if isinstance(selection, GdsLayerSelection):
                    selections.append(selection)
        return selections

    def _build_tree(self, info: GdsFileInfo) -> None:
        for cell in info.cells:
            cell_item = QTreeWidgetItem([cell.name, "", ""])
            cell_item.setFlags(cell_item.flags() | Qt.ItemFlag.ItemIsAutoTristate)
            cell_item.setCheckState(0, Qt.CheckState.Unchecked)
            _set_row_height(cell_item, self._tree.columnCount(), 28)
            if not cell.layers:
                cell_item.setDisabled(True)
                cell_item.setText(1, "0")

            for layer in cell.layers:
                selection = layer.selection
                bounds = layer.bounds
                layer_item = QTreeWidgetItem(
                    [
                        tr(
                            "gds_import.layer_datatype",
                            layer=selection.layer,
                            datatype=selection.datatype,
                        ),
                        str(layer.polygon_count),
                        (
                            f"{bounds.min_x:.2f}, {bounds.min_y:.2f} - "
                            f"{bounds.max_x:.2f}, {bounds.max_y:.2f}"
                        ),
                    ]
                )
                layer_item.setData(0, SELECTION_ROLE, selection)
                layer_item.setCheckState(0, Qt.CheckState.Unchecked)
                _set_row_height(layer_item, self._tree.columnCount(), 28)
                layer_item.setTextAlignment(1, Qt.AlignmentFlag.AlignVCenter)
                layer_item.setTextAlignment(2, Qt.AlignmentFlag.AlignVCenter)
                cell_item.addChild(layer_item)

            self._tree.addTopLevelItem(cell_item)

    def _sync_child_checks(self, item: QTreeWidgetItem, column: int) -> None:
        if column != 0 or item.childCount() == 0:
            return

        self._tree.blockSignals(True)
        try:
            state = item.checkState(0)
            if state == Qt.CheckState.PartiallyChecked:
                return
            for index in range(item.childCount()):
                child = item.child(index)
                if not child.isDisabled():
                    child.setCheckState(0, state)
        finally:
            self._tree.blockSignals(False)

    def _accept_if_selected(self) -> None:
        if self.selected_layers():
            self.accept()
            return

        QMessageBox.warning(
            self, tr("dialog.import_gds"), tr("gds_import.select_layer_warning")
        )

    def _check_only_layer_if_unambiguous(self) -> None:
        layer_items: list[QTreeWidgetItem] = []
        for top_index in range(self._tree.topLevelItemCount()):
            cell_item = self._tree.topLevelItem(top_index)
            for layer_index in range(cell_item.childCount()):
                layer_items.append(cell_item.child(layer_index))

        if len(layer_items) != 1:
            return

        layer_items[0].setCheckState(0, Qt.CheckState.Checked)


def _set_row_height(item: QTreeWidgetItem, column_count: int, height: int) -> None:
    for column in range(column_count):
        item.setSizeHint(column, QSize(0, height))
        item.setTextAlignment(column, Qt.AlignmentFlag.AlignVCenter)
