use std::path::PathBuf;

use indexmap::IndexMap;
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds2d {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
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
}

impl DisplayProperties {
    pub fn gds_layer(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visible: true,
            color: "#2D6CDF".to_owned(),
            brightness: 1.0,
            opacity: 1.0,
            z_min: 0.0,
            z_max: 15.0,
        }
    }

    pub fn baseplate(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BaseplateObject {
    pub id: String,
    pub display: DisplayProperties,
    pub bounds: Bounds2d,
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

    pub fn objects(&self) -> impl Iterator<Item = &SceneObject> {
        self.objects.values()
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn next_baseplate_name(&self) -> String {
        let count = self
            .objects()
            .filter(|obj| matches!(obj, SceneObject::Baseplate(_)))
            .count();
        t!("object.baseplate_name", index = count + 1).to_string()
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

pub fn placeholder_gds_layer(path: PathBuf) -> SceneObject {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("GDS")
        .to_owned();
    SceneObject::GdsLayer(GdsLayerObject {
        id: new_object_id(),
        display: DisplayProperties::gds_layer("L1/0"),
        file_path: path.clone(),
        source_path: path.clone(),
        source_key: stem.clone(),
        cell_name: stem,
        layer: 1,
        datatype: 0,
        bounds: Bounds2d {
            min_x: -250.0,
            min_y: -150.0,
            max_x: 250.0,
            max_y: 150.0,
        },
    })
}

pub fn new_baseplate(name: impl Into<String>, bounds: Bounds2d) -> SceneObject {
    SceneObject::Baseplate(BaseplateObject {
        id: new_object_id(),
        display: DisplayProperties::baseplate(name),
        bounds,
    })
}
