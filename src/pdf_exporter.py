from __future__ import annotations

from pathlib import Path
from tempfile import TemporaryDirectory

from reportlab.lib import colors
from reportlab.lib.pagesizes import A4
from reportlab.lib.units import mm
from reportlab.platypus import Image as RLImage
from reportlab.platypus import PageBreak, SimpleDocTemplate, Table, TableStyle

from i18n import DEFAULT_LOCALE, tr_for_locale
from objects import BaseplateObject, GdsLayerObject, SceneObject
from viewport import Viewport


def export_scene_pdf(
    file_path: Path,
    viewport: Viewport,
    objects: list[SceneObject],
    image_size: tuple[int, int] | None = None,
) -> None:
    path = file_path.expanduser().resolve()
    if path.suffix.lower() != ".pdf":
        raise ValueError(tr_for_locale(DEFAULT_LOCALE, "error.pdf_requires_pdf"))

    with TemporaryDirectory() as temp_dir:
        temp_dir_path = Path(temp_dir)
        screenshot_path = temp_dir_path / "scene.png"
        viewport.export_png(screenshot_path, image_size=image_size)
        _build_pdf(path, screenshot_path, objects)


def _build_pdf(
    file_path: Path, screenshot_path: Path, objects: list[SceneObject]
) -> None:
    doc = SimpleDocTemplate(
        str(file_path),
        pagesize=A4,
        leftMargin=14 * mm,
        rightMargin=14 * mm,
        topMargin=14 * mm,
        bottomMargin=14 * mm,
    )

    story = []

    story.append(
        RLImage(
            str(screenshot_path),
            width=doc.width,
            height=min(doc.height * 0.72, doc.width * 0.65),
            kind="proportional",
        )
    )
    story.append(PageBreak())
    story.append(_make_table(objects))

    doc.build(story)


def _make_table(objects: list[SceneObject]) -> Table:
    rows = [
        [
            _pdf_text("pdf.name"),
            _pdf_text("pdf.kind"),
            _pdf_text("pdf.cell"),
            _pdf_text("pdf.layer"),
            _pdf_text("pdf.datatype"),
            _pdf_text("pdf.x_bounds"),
            _pdf_text("pdf.y_bounds"),
            _pdf_text("pdf.z_bounds"),
        ]
    ]

    for obj in objects:
        if isinstance(obj, GdsLayerObject):
            rows.append(
                [
                    obj.name,
                    _pdf_text("pdf.gds"),
                    obj.cell_name,
                    str(obj.layer),
                    str(obj.datatype),
                    _range_text(obj.bounds.min_x, obj.bounds.max_x),
                    _range_text(obj.bounds.min_y, obj.bounds.max_y),
                    _range_text(obj.z_min, obj.z_max),
                ]
            )
        elif isinstance(obj, BaseplateObject):
            rows.append(
                [
                    obj.name,
                    _pdf_text("pdf.baseplate"),
                    "",
                    "",
                    "",
                    _range_text(obj.bounds.min_x, obj.bounds.max_x),
                    _range_text(obj.bounds.min_y, obj.bounds.max_y),
                    _range_text(obj.z_min, obj.z_max),
                ]
            )

    table = Table(rows, repeatRows=1)
    table.setStyle(
        TableStyle(
            [
                ("BACKGROUND", (0, 0), (-1, 0), colors.HexColor("#dfe6ee")),
                ("TEXTCOLOR", (0, 0), (-1, 0), colors.HexColor("#1f2328")),
                ("GRID", (0, 0), (-1, -1), 0.35, colors.HexColor("#aeb8c4")),
                ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
                ("FONTSIZE", (0, 0), (-1, -1), 7),
                ("LEADING", (0, 0), (-1, -1), 8),
                ("VALIGN", (0, 0), (-1, -1), "MIDDLE"),
                (
                    "ROWBACKGROUNDS",
                    (0, 1),
                    (-1, -1),
                    [
                        colors.white,
                        colors.HexColor("#f5f7fa"),
                    ],
                ),
            ]
        )
    )
    return table


def _range_text(min_value: float, max_value: float) -> str:
    return f"{min_value:.2f}, {max_value:.2f}"


def _pdf_text(key: str) -> str:
    return tr_for_locale(DEFAULT_LOCALE, key)
