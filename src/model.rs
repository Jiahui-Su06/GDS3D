use std::fs;
use std::path::{Path, PathBuf};

use gdsii::parser::{Element, GdsEvent, GdsParser};
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

    pub fn default_baseplate_bounds(&self) -> Bounds2d {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for obj in self.objects() {
            let bounds = obj.bounds();
            min_x = min_x.min(bounds.min_x);
            min_y = min_y.min(bounds.min_y);
            max_x = max_x.max(bounds.max_x);
            max_y = max_y.max(bounds.max_y);
        }

        if min_x.is_finite() {
            let pad = ((max_x - min_x).max(max_y - min_y) * 0.08).max(10.0);
            Bounds2d {
                min_x: min_x - pad,
                min_y: min_y - pad,
                max_x: max_x + pad,
                max_y: max_y + pad,
            }
        } else {
            Bounds2d {
                min_x: -500.0,
                min_y: -350.0,
                max_x: 500.0,
                max_y: 350.0,
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct CellGroup {
    pub key: CellKey,
    pub object_ids: Vec<String>,
}

pub fn new_object_id() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn import_gds_layers(path: &Path) -> anyhow::Result<Vec<SceneObject>> {
    let data = fs::read(path)?;
    let mut current_cell = None::<String>;
    let mut coordinate_scale = 1.0f32;
    let mut layers = IndexMap::<LayerKey, LayerGeometry>::new();

    for event in GdsParser::new(&data) {
        match event? {
            GdsEvent::LibraryBegin(library) => {
                coordinate_scale = parse_coordinate_scale(library.db_in_user)?;
            }
            GdsEvent::StructureBegin(structure) => {
                current_cell = Some(structure.name.to_owned());
            }
            GdsEvent::StructureEnd => {
                current_cell = None;
            }
            GdsEvent::Element(Element::Boundary(boundary)) => {
                let Some(cell_name) = current_cell.as_ref() else {
                    continue;
                };
                add_layer_polygon(
                    &mut layers,
                    cell_name,
                    boundary.layer,
                    boundary.datatype,
                    polygon_from_points(GdsPoint::iter_xy(boundary.xy), coordinate_scale),
                );
            }
            GdsEvent::Element(Element::Box(box_)) => {
                let Some(cell_name) = current_cell.as_ref() else {
                    continue;
                };
                add_layer_polygon(
                    &mut layers,
                    cell_name,
                    box_.layer,
                    box_.boxtype,
                    polygon_from_points(GdsPoint::iter_xy(box_.xy), coordinate_scale),
                );
            }
            GdsEvent::Element(_) | GdsEvent::Property(_) | GdsEvent::LibraryEnd => {}
        }
    }

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
        anyhow::bail!("no GDS boundary or box geometry found");
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

#[derive(Clone, Debug)]
struct LayerGeometry {
    bounds: Bounds2d,
    polygons: Vec<Polygon2d>,
}

fn add_layer_polygon(
    layers: &mut IndexMap<LayerKey, LayerGeometry>,
    cell_name: &str,
    layer: i16,
    datatype: i16,
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
        layer: i32::from(layer),
        datatype: i32::from(datatype),
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

fn is_metadata_cell(name: &str) -> bool {
    name.starts_with("$$$") && name.ends_with("$$$")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Bounds2d, Scene, SceneObject, import_gds_layers, new_baseplate};

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
}
