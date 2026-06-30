use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use gdsii::I32;
use gdsii::parser::{Aref, Element, GdsEvent, GdsParser, Path as GdsPath, Sref, Strans};
use gdsii::types::GdsPoint;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::archive::source_key_for_path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds2d {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

/// A single closed 2D polygon ring from a GDS boundary-like element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Polygon2d {
    pub points: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayProperties {
    pub name: String,
    pub visible: bool,
    pub color: String,
    pub brightness: f32,
    pub opacity: f32,
    pub z_min: f32,
    pub z_max: f32,
    #[serde(default)]
    pub defaults: DisplayDefaults,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DisplayDefaults {
    pub name: String,
    pub color: String,
    pub brightness: f32,
    pub opacity: f32,
    pub z_min: f32,
    pub z_max: f32,
}

impl DisplayProperties {
    pub fn gds_layer(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            defaults: DisplayDefaults {
                name: name.clone(),
                color: "#2D6CDF".to_owned(),
                brightness: 1.0,
                opacity: 1.0,
                z_min: 0.0,
                z_max: 15.0,
            },
            name,
            visible: true,
            color: "#2D6CDF".to_owned(),
            brightness: 1.0,
            opacity: 1.0,
            z_min: 0.0,
            z_max: 15.0,
        }
    }

    pub fn baseplate(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            defaults: DisplayDefaults {
                name: name.clone(),
                color: "#5F6B78".to_owned(),
                brightness: 1.0,
                opacity: 1.0,
                z_min: -20.0,
                z_max: 0.0,
            },
            name,
            visible: true,
            color: "#5F6B78".to_owned(),
            brightness: 1.0,
            opacity: 1.0,
            z_min: -20.0,
            z_max: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GdsLayerObject {
    pub id: String,
    pub display: DisplayProperties,
    pub file_path: PathBuf,
    pub source_path: PathBuf,
    pub source_key: String,
    pub cell_name: String,
    pub layer: i32,
    pub datatype: i32,
    pub bounds: Bounds2d,
    #[serde(default)]
    pub polygons: Vec<Polygon2d>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BaseplateObject {
    pub id: String,
    pub display: DisplayProperties,
    pub bounds: Bounds2d,
    #[serde(default)]
    pub default_bounds: Option<Bounds2d>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum SceneObject {
    GdsLayer(GdsLayerObject),
    Baseplate(BaseplateObject),
}

impl SceneObject {
    pub fn id(&self) -> &str {
        match self {
            SceneObject::GdsLayer(obj) => &obj.id,
            SceneObject::Baseplate(obj) => &obj.id,
        }
    }

    pub fn display(&self) -> &DisplayProperties {
        match self {
            SceneObject::GdsLayer(obj) => &obj.display,
            SceneObject::Baseplate(obj) => &obj.display,
        }
    }

    pub fn display_mut(&mut self) -> &mut DisplayProperties {
        match self {
            SceneObject::GdsLayer(obj) => &mut obj.display,
            SceneObject::Baseplate(obj) => &mut obj.display,
        }
    }

    pub fn bounds(&self) -> &Bounds2d {
        match self {
            SceneObject::GdsLayer(obj) => &obj.bounds,
            SceneObject::Baseplate(obj) => &obj.bounds,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.display().visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.display_mut().visible = visible;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Scene,
    Object(String),
    Cell(CellKey),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CellKey {
    pub file_path: PathBuf,
    pub cell_name: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scene {
    objects: IndexMap<String, SceneObject>,
    #[serde(skip)]
    revision: u64,
}

impl Scene {
    pub fn add(&mut self, obj: SceneObject) -> anyhow::Result<()> {
        let id = obj.id().to_owned();
        if self.objects.contains_key(&id) {
            anyhow::bail!("duplicate object id: {id}");
        }
        self.objects.insert(id, obj);
        self.touch();
        Ok(())
    }

    pub fn remove(&mut self, object_id: &str) -> Option<SceneObject> {
        let removed = self.objects.shift_remove(object_id);
        if removed.is_some() {
            self.touch();
        }
        removed
    }

    pub fn get(&self, object_id: &str) -> Option<&SceneObject> {
        self.objects.get(object_id)
    }

    pub fn get_mut(&mut self, object_id: &str) -> Option<&mut SceneObject> {
        self.objects.get_mut(object_id)
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn objects(&self) -> impl Iterator<Item = &SceneObject> {
        self.objects.values()
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn next_baseplate_name(&self) -> String {
        let prefix = "Baseplate ";
        let mut used_indices = std::collections::HashSet::new();
        for obj in self.objects() {
            let SceneObject::Baseplate(baseplate) = obj else {
                continue;
            };
            let Some(suffix) = baseplate.display.name.strip_prefix(prefix) else {
                continue;
            };
            let Ok(index) = suffix.parse::<usize>() else {
                continue;
            };
            used_indices.insert(index);
        }

        let mut index = 1;
        while used_indices.contains(&index) {
            index += 1;
        }
        format!("{prefix}{index}")
    }

    pub fn cell_groups(&self) -> Vec<CellGroup> {
        let mut groups: IndexMap<CellKey, Vec<String>> = IndexMap::new();
        for obj in self.objects() {
            if let SceneObject::GdsLayer(layer) = obj {
                let key = CellKey {
                    file_path: layer.file_path.clone(),
                    cell_name: layer.cell_name.clone(),
                };
                groups.entry(key).or_default().push(layer.id.clone());
            }
        }

        groups
            .into_iter()
            .map(|(key, object_ids)| CellGroup { key, object_ids })
            .collect()
    }

    pub fn default_baseplate_bounds(&self, selection: &Selection) -> Bounds2d {
        if let Selection::Cell(key) = selection
            && let Some(bounds) = self.bounds_for_cell(key)
        {
            return bounds;
        }

        self.gds_bounds().unwrap_or(Bounds2d {
            min_x: -100.0,
            min_y: -100.0,
            max_x: 100.0,
            max_y: 100.0,
        })
    }

    fn bounds_for_cell(&self, key: &CellKey) -> Option<Bounds2d> {
        let mut bounds = None;
        for obj in self.objects() {
            let SceneObject::GdsLayer(layer) = obj else {
                continue;
            };
            if layer.file_path != key.file_path || layer.cell_name != key.cell_name {
                continue;
            }
            merge_optional_bounds(&mut bounds, &layer.bounds);
        }
        bounds
    }

    fn gds_bounds(&self) -> Option<Bounds2d> {
        let mut bounds = None;
        for obj in self.objects() {
            let SceneObject::GdsLayer(layer) = obj else {
                continue;
            };
            merge_optional_bounds(&mut bounds, &layer.bounds);
        }
        bounds
    }
}

#[derive(Clone, Debug)]
pub struct CellGroup {
    pub key: CellKey,
    pub object_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct GdsFileInfo {
    pub file_path: PathBuf,
    pub cells: Vec<GdsCellInfo>,
}

#[derive(Clone, Debug)]
pub struct GdsCellInfo {
    pub name: String,
    pub layers: Vec<GdsLayerInfo>,
}

#[derive(Clone, Debug)]
pub struct GdsLayerInfo {
    pub selection: GdsLayerSelection,
    pub polygon_count: usize,
    pub bounds: Bounds2d,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GdsLayerSelection {
    pub cell_name: String,
    pub layer: i32,
    pub datatype: i32,
}

pub fn new_object_id() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn inspect_gds_file(path: &Path) -> anyhow::Result<GdsFileInfo> {
    let parsed = parse_gds_layers(path)?;
    let display_cells = parsed.display_cells();
    let mut cells = Vec::new();

    for cell_name in display_cells {
        let mut layers = parsed
            .flatten_cell_layers(&cell_name)?
            .iter()
            .map(|(key, geometry)| GdsLayerInfo {
                selection: GdsLayerSelection {
                    cell_name: key.cell_name.clone(),
                    layer: key.layer,
                    datatype: key.datatype,
                },
                polygon_count: geometry.polygons.len(),
                bounds: geometry.bounds.clone(),
            })
            .collect::<Vec<_>>();
        layers.sort_by_key(|layer| (layer.selection.layer, layer.selection.datatype));
        if !layers.is_empty() {
            cells.push(GdsCellInfo {
                name: cell_name,
                layers,
            });
        }
    }

    if cells.is_empty() {
        anyhow::bail!("no renderable GDS layers found");
    }

    Ok(GdsFileInfo {
        file_path: path.to_path_buf(),
        cells,
    })
}

pub fn import_gds_layers(path: &Path) -> anyhow::Result<Vec<SceneObject>> {
    let parsed = parse_gds_layers(path)?;
    objects_from_layers(path, parsed.flatten_display_layers()?)
}

pub fn import_gds_layer_selections(
    path: &Path,
    selections: &[GdsLayerSelection],
) -> anyhow::Result<Vec<SceneObject>> {
    if selections.is_empty() {
        return Ok(Vec::new());
    }

    let parsed = parse_gds_layers(path)?;
    let selected = selections.iter().cloned().collect::<HashSet<_>>();
    let mut layers = IndexMap::new();
    for cell_name in parsed.display_cells() {
        layers.extend(parsed.flatten_cell_layers(&cell_name)?.into_iter().filter(
            |(key, _geometry)| {
                selected.contains(&GdsLayerSelection {
                    cell_name: key.cell_name.clone(),
                    layer: key.layer,
                    datatype: key.datatype,
                })
            },
        ));
    }
    objects_from_layers(path, layers)
}

fn parse_gds_layers(path: &Path) -> anyhow::Result<ParsedGdsLayers> {
    let data = fs::read(path)?;
    let mut current_cell = None::<String>;
    let mut coordinate_scale = 1.0f32;
    let mut cells = IndexMap::<String, ParsedGdsCell>::new();
    let mut referenced_cells = HashSet::new();

    for event in GdsParser::new(&data) {
        match event? {
            GdsEvent::LibraryBegin(library) => {
                coordinate_scale = parse_coordinate_scale(library.db_in_user)?;
            }
            GdsEvent::StructureBegin(structure) => {
                let cell_name = structure.name.to_owned();
                cells.entry(cell_name.clone()).or_default();
                current_cell = Some(cell_name);
            }
            GdsEvent::StructureEnd => {
                current_cell = None;
            }
            GdsEvent::Element(Element::Boundary(boundary)) => {
                let Some(cell_name) = current_cell.as_ref() else {
                    continue;
                };
                let Some(cell) = cells.get_mut(cell_name) else {
                    continue;
                };
                add_cell_polygon(
                    cell,
                    boundary.layer,
                    boundary.datatype,
                    polygon_from_points(GdsPoint::iter_xy(boundary.xy), coordinate_scale),
                );
            }
            GdsEvent::Element(Element::Path(path)) => {
                let Some(cell_name) = current_cell.as_ref() else {
                    continue;
                };
                let Some(cell) = cells.get_mut(cell_name) else {
                    continue;
                };
                add_cell_polygon(
                    cell,
                    path.layer,
                    path.datatype,
                    polygon_from_path(&path, coordinate_scale),
                );
            }
            GdsEvent::Element(Element::Box(box_)) => {
                let Some(cell_name) = current_cell.as_ref() else {
                    continue;
                };
                let Some(cell) = cells.get_mut(cell_name) else {
                    continue;
                };
                add_cell_polygon(
                    cell,
                    box_.layer,
                    box_.boxtype,
                    polygon_from_points(GdsPoint::iter_xy(box_.xy), coordinate_scale),
                );
            }
            GdsEvent::Element(Element::Sref(sref)) => {
                let Some(cell_name) = current_cell.as_ref() else {
                    continue;
                };
                let Some(cell) = cells.get_mut(cell_name) else {
                    continue;
                };
                referenced_cells.insert(sref.sname.to_owned());
                if let Some(reference) = CellReference::from_sref(&sref, coordinate_scale) {
                    cell.references.push(reference);
                }
            }
            GdsEvent::Element(Element::Aref(aref)) => {
                let Some(cell_name) = current_cell.as_ref() else {
                    continue;
                };
                let Some(cell) = cells.get_mut(cell_name) else {
                    continue;
                };
                referenced_cells.insert(aref.sname.to_owned());
                if let Some(reference) = CellReference::from_aref(&aref, coordinate_scale) {
                    cell.references.push(reference);
                }
            }
            GdsEvent::Element(_) | GdsEvent::Property(_) | GdsEvent::LibraryEnd => {}
        }
    }

    if cells.values().all(ParsedGdsCell::is_empty) {
        anyhow::bail!("no GDS boundary, path, or box geometry found");
    }

    Ok(ParsedGdsLayers {
        cells,
        referenced_cells,
    })
}

fn objects_from_layers(
    path: &Path,
    layers: IndexMap<LayerKey, LayerGeometry>,
) -> anyhow::Result<Vec<SceneObject>> {
    let source_key = source_key_for_path(path);
    let mut objects = Vec::new();
    for (key, geometry) in layers {
        objects.push(SceneObject::GdsLayer(GdsLayerObject {
            id: new_object_id(),
            display: DisplayProperties::gds_layer(format!("L{}/{}", key.layer, key.datatype)),
            file_path: path.to_path_buf(),
            source_path: path.to_path_buf(),
            source_key: source_key.clone(),
            cell_name: key.cell_name,
            layer: key.layer,
            datatype: key.datatype,
            bounds: geometry.bounds,
            polygons: geometry.polygons,
        }));
    }

    if objects.is_empty() {
        anyhow::bail!("no GDS boundary, path, or box geometry found");
    }

    Ok(objects)
}

pub fn new_baseplate(name: impl Into<String>, bounds: Bounds2d) -> SceneObject {
    SceneObject::Baseplate(BaseplateObject {
        id: new_object_id(),
        display: DisplayProperties::baseplate(name),
        default_bounds: Some(bounds.clone()),
        bounds,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct LayerKey {
    cell_name: String,
    layer: i32,
    datatype: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LayerPair {
    layer: i32,
    datatype: i32,
}

#[derive(Clone, Debug)]
struct LayerGeometry {
    bounds: Bounds2d,
    polygons: Vec<Polygon2d>,
}

#[derive(Clone, Debug, Default)]
struct ParsedGdsCell {
    layers: IndexMap<LayerPair, LayerGeometry>,
    references: Vec<CellReference>,
}

impl ParsedGdsCell {
    fn is_empty(&self) -> bool {
        self.layers.is_empty() && self.references.is_empty()
    }
}

struct ParsedGdsLayers {
    cells: IndexMap<String, ParsedGdsCell>,
    referenced_cells: HashSet<String>,
}

impl ParsedGdsLayers {
    fn display_cells(&self) -> Vec<String> {
        let mut cells = self
            .cells
            .keys()
            .filter(|name| !self.referenced_cells.contains(*name))
            .filter(|name| !is_metadata_cell(name))
            .cloned()
            .collect::<Vec<_>>();
        if cells.is_empty() {
            cells = self
                .cells
                .keys()
                .filter(|name| !is_metadata_cell(name))
                .cloned()
                .collect();
        }
        cells.sort_by_key(|name| name.to_lowercase());
        cells
    }

    fn flatten_display_layers(&self) -> anyhow::Result<IndexMap<LayerKey, LayerGeometry>> {
        let mut layers = IndexMap::new();
        for cell_name in self.display_cells() {
            layers.extend(self.flatten_cell_layers(&cell_name)?);
        }
        Ok(layers)
    }

    fn flatten_cell_layers(
        &self,
        cell_name: &str,
    ) -> anyhow::Result<IndexMap<LayerKey, LayerGeometry>> {
        let mut layers = IndexMap::new();
        let mut stack = Vec::new();
        self.flatten_cell_into(
            cell_name,
            cell_name,
            Transform2d::identity(),
            &mut stack,
            &mut layers,
        )?;
        Ok(layers)
    }

    fn flatten_cell_into(
        &self,
        cell_name: &str,
        output_cell_name: &str,
        transform: Transform2d,
        stack: &mut Vec<String>,
        layers: &mut IndexMap<LayerKey, LayerGeometry>,
    ) -> anyhow::Result<()> {
        const DEPTH_MAX: usize = 512;

        if stack.len() >= DEPTH_MAX {
            anyhow::bail!("GDS reference depth exceeds {DEPTH_MAX}");
        }
        if stack.iter().any(|name| name == cell_name) {
            anyhow::bail!("cyclic GDS reference involving cell {cell_name}");
        }
        let Some(cell) = self.cells.get(cell_name) else {
            anyhow::bail!("missing referenced GDS cell {cell_name}");
        };

        stack.push(cell_name.to_owned());
        for (pair, geometry) in &cell.layers {
            for polygon in &geometry.polygons {
                add_layer_polygon(
                    layers,
                    output_cell_name,
                    pair.layer,
                    pair.datatype,
                    transform_polygon(polygon, transform),
                );
            }
        }
        for reference in &cell.references {
            for reference_transform in &reference.transforms {
                self.flatten_cell_into(
                    &reference.cell_name,
                    output_cell_name,
                    transform.then(*reference_transform),
                    stack,
                    layers,
                )?;
            }
        }
        stack.pop();
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct CellReference {
    cell_name: String,
    transforms: Vec<Transform2d>,
}

impl CellReference {
    fn from_sref(sref: &Sref<'_>, coordinate_scale: f32) -> Option<Self> {
        let origin = point_from_xy(sref.xy, 0, coordinate_scale)?;
        Some(Self {
            cell_name: sref.sname.to_owned(),
            transforms: vec![Transform2d::from_strans(sref.strans).with_translation(origin)],
        })
    }

    fn from_aref(aref: &Aref<'_>, coordinate_scale: f32) -> Option<Self> {
        let columns = usize::try_from(aref.colrow.0).ok()?;
        let rows = usize::try_from(aref.colrow.1).ok()?;
        if columns == 0 || rows == 0 {
            return None;
        }

        let origin = point_from_xy(aref.xy, 0, coordinate_scale)?;
        let column_end = point_from_xy(aref.xy, 1, coordinate_scale)?;
        let row_end = point_from_xy(aref.xy, 2, coordinate_scale)?;
        let column_step = step_vector(origin, column_end, columns);
        let row_step = step_vector(origin, row_end, rows);
        let base = Transform2d::from_strans(aref.strans).with_translation(origin);
        let mut transforms = Vec::with_capacity(columns.saturating_mul(rows));

        for row in 0..rows {
            for column in 0..columns {
                transforms.push(base.with_offset([
                    column_step[0] * column as f32 + row_step[0] * row as f32,
                    column_step[1] * column as f32 + row_step[1] * row as f32,
                ]));
            }
        }

        Some(Self {
            cell_name: aref.sname.to_owned(),
            transforms,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Transform2d {
    xx: f32,
    xy: f32,
    yx: f32,
    yy: f32,
    tx: f32,
    ty: f32,
}

impl Transform2d {
    fn identity() -> Self {
        Self {
            xx: 1.0,
            xy: 0.0,
            yx: 0.0,
            yy: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn from_strans(strans: Option<Strans>) -> Self {
        let Some(strans) = strans else {
            return Self::identity();
        };
        let magnification = strans.mag.map(f64::from).unwrap_or(1.0) as f32;
        let angle = (strans.angle.map(f64::from).unwrap_or(0.0) as f32).to_radians();
        let sin = angle.sin() * magnification;
        let cos = angle.cos() * magnification;
        let reflect = if strans.reflection { -1.0 } else { 1.0 };

        Self {
            xx: cos,
            xy: -sin * reflect,
            yx: sin,
            yy: cos * reflect,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn with_translation(mut self, origin: [f32; 2]) -> Self {
        self.tx = origin[0];
        self.ty = origin[1];
        self
    }

    fn with_offset(mut self, offset: [f32; 2]) -> Self {
        self.tx += offset[0];
        self.ty += offset[1];
        self
    }

    fn then(self, next: Self) -> Self {
        Self {
            xx: self.xx * next.xx + self.xy * next.yx,
            xy: self.xx * next.xy + self.xy * next.yy,
            yx: self.yx * next.xx + self.yy * next.yx,
            yy: self.yx * next.xy + self.yy * next.yy,
            tx: self.xx * next.tx + self.xy * next.ty + self.tx,
            ty: self.yx * next.tx + self.yy * next.ty + self.ty,
        }
    }

    fn apply(self, point: [f32; 2]) -> [f32; 2] {
        [
            self.xx * point[0] + self.xy * point[1] + self.tx,
            self.yx * point[0] + self.yy * point[1] + self.ty,
        ]
    }
}

fn add_cell_polygon(
    cell: &mut ParsedGdsCell,
    layer: i16,
    datatype: i16,
    polygon: Option<Polygon2d>,
) {
    let Some(polygon) = polygon else {
        return;
    };
    let Some(bounds) = polygon_bounds(&polygon) else {
        return;
    };

    let key = LayerPair {
        layer: i32::from(layer),
        datatype: i32::from(datatype),
    };
    let layer = cell.layers.entry(key).or_insert_with(|| LayerGeometry {
        bounds: bounds.clone(),
        polygons: Vec::new(),
    });
    merge_bounds(&mut layer.bounds, &bounds);
    layer.polygons.push(polygon);
}

fn add_layer_polygon(
    layers: &mut IndexMap<LayerKey, LayerGeometry>,
    cell_name: &str,
    layer: i32,
    datatype: i32,
    polygon: Option<Polygon2d>,
) {
    if is_metadata_cell(cell_name) {
        return;
    }

    let Some(polygon) = polygon else {
        return;
    };
    let Some(bounds) = polygon_bounds(&polygon) else {
        return;
    };

    let key = LayerKey {
        cell_name: cell_name.to_owned(),
        layer,
        datatype,
    };
    let layer = layers.entry(key).or_insert_with(|| LayerGeometry {
        bounds: bounds.clone(),
        polygons: Vec::new(),
    });
    merge_bounds(&mut layer.bounds, &bounds);
    layer.polygons.push(polygon);
}

fn parse_coordinate_scale(db_in_user: f64) -> anyhow::Result<f32> {
    if !db_in_user.is_finite() || db_in_user <= 0.0 {
        anyhow::bail!("invalid GDS library unit scale: {db_in_user}");
    }
    if db_in_user > f64::from(f32::MAX) {
        anyhow::bail!("GDS library unit scale is too large: {db_in_user}");
    }
    Ok(db_in_user as f32)
}

fn point_from_xy(xy: &[I32], index: usize, coordinate_scale: f32) -> Option<[f32; 2]> {
    let point = GdsPoint::iter_xy(xy).nth(index)?;
    let x = point.x as f32 * coordinate_scale;
    let y = point.y as f32 * coordinate_scale;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some([x, y])
}

fn step_vector(origin: [f32; 2], end: [f32; 2], count: usize) -> [f32; 2] {
    let divisor = count as f32;
    [
        (end[0] - origin[0]) / divisor,
        (end[1] - origin[1]) / divisor,
    ]
}

fn polygon_from_path(path: &GdsPath<'_>, coordinate_scale: f32) -> Option<Polygon2d> {
    let width = path.width?.unsigned_abs() as f32 * coordinate_scale;
    if !width.is_finite() || width <= 0.0 {
        return None;
    }

    let points = GdsPoint::iter_xy(path.xy)
        .map(|point| {
            [
                point.x as f32 * coordinate_scale,
                point.y as f32 * coordinate_scale,
            ]
        })
        .collect::<Vec<_>>();
    path_polygon_from_points(&points, width)
}

fn path_polygon_from_points(points: &[[f32; 2]], width: f32) -> Option<Polygon2d> {
    if points.len() < 2 {
        return None;
    }

    let half_width = width * 0.5;
    let mut normals = Vec::with_capacity(points.len().saturating_sub(1));
    for segment in points.windows(2) {
        normals.push(segment_normal(segment[0], segment[1])?);
    }

    let mut left = Vec::with_capacity(points.len());
    let mut right = Vec::with_capacity(points.len());
    for index in 0..points.len() {
        let normal = if index == 0 {
            normals[0]
        } else if index == points.len() - 1 {
            normals[normals.len() - 1]
        } else {
            average_normal(normals[index - 1], normals[index])
        };
        left.push([
            points[index][0] + normal[0] * half_width,
            points[index][1] + normal[1] * half_width,
        ]);
        right.push([
            points[index][0] - normal[0] * half_width,
            points[index][1] - normal[1] * half_width,
        ]);
    }

    right.reverse();
    let mut polygon = Polygon2d { points: left };
    polygon.points.extend(right);
    polygon_bounds(&polygon)?;
    Some(polygon)
}

fn segment_normal(start: [f32; 2], end: [f32; 2]) -> Option<[f32; 2]> {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length = dx.hypot(dy);
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    Some([-dy / length, dx / length])
}

fn average_normal(previous: [f32; 2], next: [f32; 2]) -> [f32; 2] {
    let x = previous[0] + next[0];
    let y = previous[1] + next[1];
    let length = x.hypot(y);
    if !length.is_finite() || length <= f32::EPSILON {
        return next;
    }
    [x / length, y / length]
}

fn transform_polygon(polygon: &Polygon2d, transform: Transform2d) -> Option<Polygon2d> {
    let mut transformed = Polygon2d {
        points: Vec::with_capacity(polygon.points.len()),
    };
    for point in &polygon.points {
        let next = transform.apply(*point);
        if !next[0].is_finite() || !next[1].is_finite() {
            return None;
        }
        if transformed.points.last().is_some_and(|last| *last == next) {
            continue;
        }
        transformed.points.push(next);
    }

    if transformed.points.len() >= 2 && transformed.points.first() == transformed.points.last() {
        transformed.points.pop();
    }
    if transformed.points.len() < 3 {
        return None;
    }
    polygon_bounds(&transformed)?;
    Some(transformed)
}

fn polygon_from_points(
    points: impl IntoIterator<Item = GdsPoint>,
    coordinate_scale: f32,
) -> Option<Polygon2d> {
    let mut polygon = Polygon2d { points: Vec::new() };
    for point in points {
        let x = point.x as f32 * coordinate_scale;
        let y = point.y as f32 * coordinate_scale;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let next = [x, y];
        if polygon.points.last().is_some_and(|last| *last == next) {
            continue;
        }
        polygon.points.push(next);
    }

    if polygon.points.len() >= 2 && polygon.points.first() == polygon.points.last() {
        polygon.points.pop();
    }
    if polygon.points.len() < 3 {
        return None;
    }
    polygon_bounds(&polygon)?;
    Some(polygon)
}

fn polygon_bounds(polygon: &Polygon2d) -> Option<Bounds2d> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for [x, y] in &polygon.points {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }

    if min_x >= max_x || min_y >= max_y {
        return None;
    }
    Some(Bounds2d {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

fn merge_bounds(target: &mut Bounds2d, other: &Bounds2d) {
    target.min_x = target.min_x.min(other.min_x);
    target.min_y = target.min_y.min(other.min_y);
    target.max_x = target.max_x.max(other.max_x);
    target.max_y = target.max_y.max(other.max_y);
}

fn merge_optional_bounds(target: &mut Option<Bounds2d>, other: &Bounds2d) {
    match target {
        Some(bounds) => merge_bounds(bounds, other),
        None => *target = Some(other.clone()),
    }
}

fn is_metadata_cell(name: &str) -> bool {
    name.starts_with("$$$") && name.ends_with("$$$")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        Bounds2d, CellKey, DisplayProperties, GdsLayerObject, Scene, SceneObject, Selection,
        import_gds_layers, inspect_gds_file, new_baseplate, new_object_id,
    };

    #[test]
    fn imports_gds_layers() {
        let objects = import_gds_layers(Path::new("models/AWG.gds")).expect("import sample GDS");
        assert!(!objects.is_empty());
        for obj in objects {
            let SceneObject::GdsLayer(layer) = obj else {
                panic!("expected GDS layer object");
            };
            assert!(!layer.cell_name.starts_with("$$$"));
            assert!(!layer.polygons.is_empty());
            assert!(layer.bounds.min_x < layer.bounds.max_x);
            assert!(layer.bounds.min_y < layer.bounds.max_y);
        }
    }

    #[test]
    fn uses_stable_english_baseplate_names() {
        let mut scene = Scene::default();
        assert_eq!(scene.next_baseplate_name(), "Baseplate 1");

        let bounds = Bounds2d {
            min_x: -1.0,
            min_y: -1.0,
            max_x: 1.0,
            max_y: 1.0,
        };
        scene
            .add(new_baseplate("Baseplate 1", bounds.clone()))
            .expect("add first baseplate");
        scene
            .add(new_baseplate("Baseplate 3", bounds))
            .expect("add third baseplate");

        assert_eq!(scene.next_baseplate_name(), "Baseplate 2");
    }

    #[test]
    fn creates_baseplate_bounds_from_selected_cell() {
        let mut scene = Scene::default();
        let file_path = PathBuf::from("models/test.gds");
        scene
            .add(test_gds_layer(
                file_path.clone(),
                "AWG",
                Bounds2d {
                    min_x: -10.0,
                    min_y: -20.0,
                    max_x: 30.0,
                    max_y: 40.0,
                },
            ))
            .expect("add selected cell layer");
        scene
            .add(test_gds_layer(
                file_path.clone(),
                "Other",
                Bounds2d {
                    min_x: -1000.0,
                    min_y: -1000.0,
                    max_x: 1000.0,
                    max_y: 1000.0,
                },
            ))
            .expect("add other cell layer");

        let bounds = scene.default_baseplate_bounds(&Selection::Cell(CellKey {
            file_path,
            cell_name: "AWG".to_owned(),
        }));

        assert_eq!(
            bounds,
            Bounds2d {
                min_x: -10.0,
                min_y: -20.0,
                max_x: 30.0,
                max_y: 40.0,
            }
        );
    }

    #[test]
    fn creates_baseplate_bounds_from_all_gds_layers_when_scene_selected() {
        let mut scene = Scene::default();
        scene
            .add(test_gds_layer(
                PathBuf::from("models/a.gds"),
                "A",
                Bounds2d {
                    min_x: -1.0,
                    min_y: -2.0,
                    max_x: 3.0,
                    max_y: 4.0,
                },
            ))
            .expect("add first layer");
        scene
            .add(test_gds_layer(
                PathBuf::from("models/b.gds"),
                "B",
                Bounds2d {
                    min_x: -10.0,
                    min_y: 5.0,
                    max_x: 20.0,
                    max_y: 30.0,
                },
            ))
            .expect("add second layer");

        let bounds = scene.default_baseplate_bounds(&Selection::Scene);

        assert_eq!(
            bounds,
            Bounds2d {
                min_x: -10.0,
                min_y: -2.0,
                max_x: 20.0,
                max_y: 30.0,
            }
        );
    }

    #[test]
    fn ignores_existing_baseplates_when_creating_default_bounds() {
        let mut scene = Scene::default();
        scene
            .add(new_baseplate(
                "Baseplate 1",
                Bounds2d {
                    min_x: -1000.0,
                    min_y: -1000.0,
                    max_x: 1000.0,
                    max_y: 1000.0,
                },
            ))
            .expect("add existing baseplate");

        let bounds = scene.default_baseplate_bounds(&Selection::Scene);

        assert_eq!(
            bounds,
            Bounds2d {
                min_x: -100.0,
                min_y: -100.0,
                max_x: 100.0,
                max_y: 100.0,
            }
        );
    }

    #[test]
    fn inspects_top_level_gds_cells() {
        let info = inspect_gds_file(Path::new("models/AWG_0.8nmCS_16CH_0nmOS.gds"))
            .expect("inspect sample GDS");

        assert!(info.cells.iter().any(|cell| cell.name == "AWG"));
        assert!(
            !info
                .cells
                .iter()
                .any(|cell| cell.name.starts_with("straight_gdsfactory"))
        );

        let awg = info
            .cells
            .iter()
            .find(|cell| cell.name == "AWG")
            .expect("AWG top cell");
        let layer = awg
            .layers
            .iter()
            .find(|layer| layer.selection.layer == 4 && layer.selection.datatype == 1)
            .expect("AWG L4/1 layer");
        assert!(
            layer.polygon_count > 2,
            "expected referenced child geometry to be flattened into AWG"
        );
    }

    fn test_gds_layer(file_path: PathBuf, cell_name: &str, bounds: Bounds2d) -> SceneObject {
        SceneObject::GdsLayer(GdsLayerObject {
            id: new_object_id(),
            display: DisplayProperties::gds_layer("L4/1"),
            file_path: file_path.clone(),
            source_path: file_path,
            source_key: String::new(),
            cell_name: cell_name.to_owned(),
            layer: 4,
            datatype: 1,
            bounds,
            polygons: Vec::new(),
        })
    }
}
