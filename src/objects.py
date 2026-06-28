from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal, TypeAlias
from uuid import uuid4


ObjectKind: TypeAlias = Literal["gds_layer", "baseplate"]


def new_object_id() -> str:
    return uuid4().hex


@dataclass(frozen=True)
class Bounds2D:
    min_x: float
    min_y: float
    max_x: float
    max_y: float

    def __post_init__(self) -> None:
        if self.min_x >= self.max_x:
            raise ValueError("min_x must be smaller than max_x")
        if self.min_y >= self.max_y:
            raise ValueError("min_y must be smaller than max_y")

    @property
    def width(self) -> float:
        return self.max_x - self.min_x

    @property
    def height(self) -> float:
        return self.max_y - self.min_y


@dataclass
class GdsLayerObject:
    name: str
    file_path: Path
    cell_name: str
    layer: int
    datatype: int
    bounds: Bounds2D
    z_min: float = 0.0
    z_max: float = 15.0
    color: str = "#2D6CDF"
    brightness: float = 1.0
    opacity: float = 1.0
    visible: bool = True
    defaults: dict[str, Any] = field(default_factory=dict)
    id: str = field(default_factory=new_object_id)
    kind: Literal["gds_layer"] = "gds_layer"

    def __post_init__(self) -> None:
        if self.z_min >= self.z_max:
            raise ValueError("z_min must be smaller than z_max")
        if not 0.0 <= self.opacity <= 1.0:
            raise ValueError("opacity must be between 0 and 1")
        if not 0.0 <= self.brightness <= 2.0:
            raise ValueError("brightness must be between 0 and 2")
        if not self.defaults:
            self.defaults = {
                "name": self.name,
                "color": self.color,
                "brightness": self.brightness,
                "opacity": self.opacity,
                "z_min": self.z_min,
                "z_max": self.z_max,
            }


@dataclass
class BaseplateObject:
    name: str
    bounds: Bounds2D
    z_min: float = -50.0
    z_max: float = 0.0
    color: str = "#5F6B78"
    brightness: float = 1.0
    opacity: float = 1.0
    visible: bool = True
    defaults: dict[str, Any] = field(default_factory=dict)
    id: str = field(default_factory=new_object_id)
    kind: Literal["baseplate"] = "baseplate"

    def __post_init__(self) -> None:
        if self.z_min >= self.z_max:
            raise ValueError("z_min must be smaller than z_max")
        if not 0.0 <= self.opacity <= 1.0:
            raise ValueError("opacity must be between 0 and 1")
        if not 0.0 <= self.brightness <= 2.0:
            raise ValueError("brightness must be between 0 and 2")
        if not self.defaults:
            self.defaults = {
                "name": self.name,
                "color": self.color,
                "brightness": self.brightness,
                "opacity": self.opacity,
                "min_x": self.bounds.min_x,
                "min_y": self.bounds.min_y,
                "max_x": self.bounds.max_x,
                "max_y": self.bounds.max_y,
                "z_min": self.z_min,
                "z_max": self.z_max,
            }


SceneObject: TypeAlias = GdsLayerObject | BaseplateObject
