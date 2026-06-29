from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Literal

from PySide6.QtCore import QSize, Qt, Signal
from PySide6.QtGui import QIcon
from PySide6.QtWidgets import (
    QAbstractItemView,
    QHeaderView,
    QMenu,
    QTreeWidget,
    QTreeWidgetItem,
)

from i18n import tr
from objects import SceneObject


OBJECT_ID_ROLE = Qt.ItemDataRole.UserRole
VISIBLE_ROLE = Qt.ItemDataRole.UserRole + 1
GROUP_KEY_ROLE = Qt.ItemDataRole.UserRole + 2
GROUP_KIND_ROLE = Qt.ItemDataRole.UserRole + 3
GROUP_NAME_ROLE = Qt.ItemDataRole.UserRole + 4
GROUP_FILE_ROLE = Qt.ItemDataRole.UserRole + 5

ICON_DIR = Path(__file__).resolve().parent / "icons"
EYE_ICON = QIcon(str(ICON_DIR / "eye.svg"))
EYE_OFF_ICON = QIcon(str(ICON_DIR / "eye_off.svg"))


@dataclass(frozen=True)
class ComponentGroupInfo:
    kind: Literal["cell"]
    name: str
    file_path: Path
    object_count: int
    object_ids: tuple[str, ...]


class ComponentTree(QTreeWidget):
    object_selected = Signal(object)
    visibility_changed = Signal(str, bool)
    group_visibility_changed = Signal(object, bool)
    delete_requested = Signal(object)

    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self.setColumnCount(2)
        self.setHeaderHidden(True)
        self.setIndentation(16)
        self.setUniformRowHeights(True)
        self.setRootIsDecorated(True)
        self.setIconSize(QSize(18, 18))
        self.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self.setTextElideMode(Qt.TextElideMode.ElideRight)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        header = self.header()
        header.setStretchLastSection(False)
        header.setSectionResizeMode(0, QHeaderView.ResizeMode.Stretch)
        header.setSectionResizeMode(1, QHeaderView.ResizeMode.Fixed)
        header.resizeSection(1, 24)

        self._root = QTreeWidgetItem([tr("scene.root"), ""])
        self._root.setData(0, OBJECT_ID_ROLE, None)
        self._root.setExpanded(True)

        self.currentItemChanged.connect(self._emit_current_object)
        self.itemClicked.connect(self._handle_item_clicked)
        self.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.customContextMenuRequested.connect(self._show_context_menu)

    def add_object(self, obj: SceneObject) -> None:
        item = QTreeWidgetItem([self._label_for(obj), ""])
        item.setData(0, OBJECT_ID_ROLE, obj.id)
        item.setData(1, VISIBLE_ROLE, obj.visible)
        item.setIcon(1, EYE_ICON if obj.visible else EYE_OFF_ICON)
        item.setToolTip(0, self._label_for(obj))
        item.setTextAlignment(1, Qt.AlignmentFlag.AlignCenter)

        parent = self._parent_for(obj)
        if parent is self._root:
            self.addTopLevelItem(item)
        else:
            parent.addChild(item)
        parent.setExpanded(True)
        self.setCurrentItem(item)

    def remove_object(self, object_id: str) -> None:
        item = self._find_item(object_id)
        if item is None:
            return
        parent = item.parent()
        if parent is None:
            top_index = self.indexOfTopLevelItem(item)
            if top_index >= 0:
                self.takeTopLevelItem(top_index)
        else:
            parent.removeChild(item)
            self._remove_empty_groups(parent)
        self.setCurrentItem(None)

    def refresh_object(self, obj: SceneObject) -> None:
        item = self._find_item(obj.id)
        if item is not None:
            label = self._label_for(obj)
            item.setText(0, label)
            item.setToolTip(0, label)
            item.setData(1, VISIBLE_ROLE, obj.visible)
            item.setIcon(1, EYE_ICON if obj.visible else EYE_OFF_ICON)

    def refresh_text(self) -> None:
        self._root.setText(0, tr("scene.root"))

    def current_object_id(self) -> str | None:
        item = self.currentItem()
        if item is None:
            return None
        value = item.data(0, OBJECT_ID_ROLE)
        return value if isinstance(value, str) else None

    def select_object(self, object_id: str | None) -> None:
        if object_id is None:
            self.setCurrentItem(None)
            return
        item = self._find_item(object_id)
        if item is not None:
            self.setCurrentItem(item)

    def group_info_for_item(
        self, item: QTreeWidgetItem | None
    ) -> ComponentGroupInfo | None:
        if item is None:
            return None
        return self._group_info(item)

    def _emit_current_object(self, current: QTreeWidgetItem | None, _previous) -> None:
        if current is None:
            self.object_selected.emit(None)
            return
        value = current.data(0, OBJECT_ID_ROLE)
        if isinstance(value, str):
            self.object_selected.emit(value)
            return

        group = self._group_info(current)
        self.object_selected.emit(group)

    def _handle_item_clicked(self, item: QTreeWidgetItem, column: int) -> None:
        if column != 1:
            return
        object_id = item.data(0, OBJECT_ID_ROLE)
        if not isinstance(object_id, str):
            return
        current = bool(item.data(1, VISIBLE_ROLE))
        self.visibility_changed.emit(object_id, not current)

    def _show_context_menu(self, position) -> None:
        item = self.itemAt(position)
        if item is None:
            return

        menu = QMenu(self.window())
        object_id = item.data(0, OBJECT_ID_ROLE)
        group = self._group_info(item)

        if isinstance(object_id, str):
            visible = bool(item.data(1, VISIBLE_ROLE))
            label = tr("context.show") if not visible else tr("context.hide")
            menu.addAction(
                label,
                lambda: self.visibility_changed.emit(object_id, not visible),
            )
            menu.addAction(
                tr("action.delete"), lambda: self.delete_requested.emit(object_id)
            )
        elif group is not None:
            visible = self._group_has_visible_child(item)
            label = tr("context.show_cell") if not visible else tr("context.hide_cell")
            menu.addAction(
                label,
                lambda: self.group_visibility_changed.emit(group, not visible),
            )
            menu.addAction(
                tr("context.delete_cell"), lambda: self.delete_requested.emit(group)
            )

        if not menu.actions():
            return

        menu.exec(self.viewport().mapToGlobal(position))

    def _find_item(self, object_id: str) -> QTreeWidgetItem | None:
        pending = self._top_level_items()
        while pending:
            item = pending.pop()
            if item.data(0, OBJECT_ID_ROLE) == object_id:
                return item
            for index in range(item.childCount()):
                pending.append(item.child(index))
        return None

    def _label_for(self, obj: SceneObject) -> str:
        return obj.name

    def _parent_for(self, obj: SceneObject) -> QTreeWidgetItem:
        if obj.kind != "gds_layer":
            return self._root

        return self._find_or_create_group(
            self._root,
            f"cell:{obj.file_path}:{obj.cell_name}",
            kind="cell",
            name=obj.cell_name,
            file_path=obj.file_path,
        )

    def _find_or_create_group(
        self,
        parent: QTreeWidgetItem,
        key: str,
        kind: Literal["cell"],
        name: str,
        file_path: Path,
    ) -> QTreeWidgetItem:
        for item in self._children_for(parent):
            if item.data(0, GROUP_KEY_ROLE) == key:
                return item

        item = QTreeWidgetItem([name, ""])
        item.setData(0, OBJECT_ID_ROLE, None)
        item.setData(0, GROUP_KEY_ROLE, key)
        item.setData(0, GROUP_KIND_ROLE, kind)
        item.setData(0, GROUP_NAME_ROLE, name)
        item.setData(0, GROUP_FILE_ROLE, file_path)
        item.setToolTip(0, name)
        if parent is self._root:
            self.addTopLevelItem(item)
        else:
            parent.addChild(item)
        return item

    def _children_for(self, parent: QTreeWidgetItem) -> list[QTreeWidgetItem]:
        if parent is self._root:
            return self._top_level_items()
        return [parent.child(index) for index in range(parent.childCount())]

    def _top_level_items(self) -> list[QTreeWidgetItem]:
        return [self.topLevelItem(index) for index in range(self.topLevelItemCount())]

    def _remove_empty_groups(self, item: QTreeWidgetItem) -> None:
        current = item
        while current is not self._root and current.childCount() == 0:
            parent = current.parent()
            if parent is None:
                top_index = self.indexOfTopLevelItem(current)
                if top_index >= 0:
                    self.takeTopLevelItem(top_index)
                return
            parent.removeChild(current)
            current = parent

    def _group_info(self, item: QTreeWidgetItem) -> ComponentGroupInfo | None:
        kind = item.data(0, GROUP_KIND_ROLE)
        name = item.data(0, GROUP_NAME_ROLE)
        file_path = item.data(0, GROUP_FILE_ROLE)
        if kind == "cell" and isinstance(name, str) and isinstance(file_path, Path):
            object_ids: list[str] = []
            for index in range(item.childCount()):
                object_id = item.child(index).data(0, OBJECT_ID_ROLE)
                if isinstance(object_id, str):
                    object_ids.append(object_id)
            return ComponentGroupInfo(
                kind="cell",
                name=name,
                file_path=file_path,
                object_count=item.childCount(),
                object_ids=tuple(object_ids),
            )
        return None

    def _group_has_visible_child(self, item: QTreeWidgetItem) -> bool:
        for index in range(item.childCount()):
            child = item.child(index)
            if bool(child.data(1, VISIBLE_ROLE)):
                return True
        return False
