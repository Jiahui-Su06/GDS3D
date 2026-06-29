from __future__ import annotations

from collections import OrderedDict

from i18n import tr
from objects import SceneObject


class Scene:
    """Owns scene objects in stable display order."""

    def __init__(self) -> None:
        self._objects: OrderedDict[str, SceneObject] = OrderedDict()

    def add(self, obj: SceneObject) -> None:
        if obj.id in self._objects:
            raise ValueError(tr("error.duplicate_object_id", object_id=obj.id))
        self._objects[obj.id] = obj

    def remove(self, object_id: str) -> SceneObject:
        try:
            return self._objects.pop(object_id)
        except KeyError as exc:
            raise KeyError(tr("error.object_not_found", object_id=object_id)) from exc

    def get(self, object_id: str) -> SceneObject | None:
        return self._objects.get(object_id)

    def objects(self) -> list[SceneObject]:
        return list(self._objects.values())

    def count(self) -> int:
        return len(self._objects)
