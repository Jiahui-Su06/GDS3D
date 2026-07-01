use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use crc32fast::Hasher as Crc32Hasher;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::model::SceneObject;

pub const ARCHIVE_FORMAT_VERSION: u32 = 1;
pub const SCENE_JSON_NAME: &str = "scene.json";
pub const RAW_GDS_DIR: &str = "gds";

const ZIP_LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4b50;
const ZIP_CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4b50;
const ZIP_END_SIGNATURE: u32 = 0x0605_4b50;
const ZIP_STORE_METHOD: u16 = 0;
const ZIP_DEFLATE_METHOD: u16 = 8;
const ZIP_VERSION_NEEDED: u16 = 20;
const ZIP_VERSION_MADE_BY: u16 = 20;
const ZIP_SEARCH_BACK_MAX: usize = 65_557;
const ZIP_ENTRY_BYTES_MAX: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveObject {
    pub kind: String,
    pub payload: Value,
}

pub type ArchiveData = (Vec<ArchiveObject>, HashMap<String, Vec<u8>>);

/// Write a `.gds3d` project archive with `scene.json` and embedded GDS sources.
pub fn write_archive(file_path: &Path, objects: &[SceneObject]) -> anyhow::Result<()> {
    let path = normalize_output_path(file_path)?;
    let mut entries = Vec::new();
    let scene_payload = Value::Object(build_scene_payload(objects));
    entries.push(ZipEntry::from_json(SCENE_JSON_NAME, &scene_payload)?);
    write_gds_sources(&mut entries, objects)?;
    write_zip_file(&path, &entries)
}

/// Read a `.gds3d` project archive.
pub fn read_archive(file_path: &Path) -> anyhow::Result<ArchiveData> {
    if file_path.extension().and_then(|suffix| suffix.to_str()) != Some("gds3d") {
        bail!("selected file is not a .gds3d archive");
    }

    let entries = read_zip_file(file_path)?;
    let mut objects = None;
    let mut gds_sources = HashMap::new();

    for entry in entries {
        if entry.name == SCENE_JSON_NAME {
            let scene: SceneArchive =
                serde_json::from_slice(&entry.data).context("parse scene.json")?;
            if scene.format_version != ARCHIVE_FORMAT_VERSION {
                bail!(
                    "unsupported project archive version: {}",
                    scene.format_version
                );
            }
            objects = Some(
                scene
                    .objects
                    .into_iter()
                    .map(|item| ArchiveObject {
                        kind: item.kind,
                        payload: item.payload,
                    })
                    .collect(),
            );
            continue;
        }

        if let Some(source_name) = entry.name.strip_prefix(&format!("{RAW_GDS_DIR}/"))
            && !source_name.is_empty()
            && safe_archive_name(source_name)
        {
            gds_sources.insert(source_name.to_owned(), entry.data);
        }
    }

    let objects = objects.ok_or_else(|| anyhow!("project archive is missing scene.json"))?;
    Ok((objects, gds_sources))
}

pub fn source_key_for_path(path: &Path) -> String {
    let display_path = path.to_string_lossy();
    let mut hasher = Crc32Hasher::new();
    hasher.update(display_path.as_bytes());
    let digest = hasher.finalize();
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("GDS");
    let suffix = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{}", extension.to_ascii_lowercase()))
        .unwrap_or_default();
    format!("{stem}-{digest:08x}{suffix}")
}

fn normalize_output_path(file_path: &Path) -> anyhow::Result<PathBuf> {
    if file_path.extension().and_then(|suffix| suffix.to_str()) == Some("gds3d") {
        return Ok(file_path.to_path_buf());
    }
    bail!("project archive requires .gds3d suffix");
}

fn build_scene_payload(objects: &[SceneObject]) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(
        "format_version".to_owned(),
        Value::from(ARCHIVE_FORMAT_VERSION),
    );
    map.insert(
        "objects".to_owned(),
        Value::Array(objects.iter().map(serialize_object).collect()),
    );
    map
}

fn serialize_object(obj: &SceneObject) -> Value {
    let mut payload = Map::new();
    match obj {
        SceneObject::GdsLayer(layer) => {
            payload.insert("object_id".to_owned(), Value::from(layer.id.clone()));
            payload.insert("name".to_owned(), Value::from(layer.display.name.clone()));
            payload.insert(
                "source_key".to_owned(),
                Value::from(layer.source_key.clone()),
            );
            payload.insert(
                "source_name".to_owned(),
                Value::from(file_name(&layer.source_path)),
            );
            payload.insert("cell_name".to_owned(), Value::from(layer.cell_name.clone()));
            payload.insert("layer".to_owned(), Value::from(layer.layer));
            payload.insert("datatype".to_owned(), Value::from(layer.datatype));
            payload.insert(
                "display_path".to_owned(),
                Value::from(layer.file_path.display().to_string()),
            );
            payload.insert(
                "bounds".to_owned(),
                serde_json::to_value(&layer.bounds).unwrap_or(Value::Null),
            );
            payload.insert("z_min".to_owned(), Value::from(layer.display.z_min));
            payload.insert("z_max".to_owned(), Value::from(layer.display.z_max));
            payload.insert("color".to_owned(), Value::from(layer.display.color.clone()));
            payload.insert(
                "brightness".to_owned(),
                Value::from(layer.display.brightness),
            );
            payload.insert("visible".to_owned(), Value::from(layer.display.visible));
            archive_object("gds_layer", payload)
        }
        SceneObject::Baseplate(baseplate) => {
            payload.insert("object_id".to_owned(), Value::from(baseplate.id.clone()));
            payload.insert(
                "name".to_owned(),
                Value::from(baseplate.display.name.clone()),
            );
            payload.insert(
                "bounds".to_owned(),
                serde_json::to_value(&baseplate.bounds).unwrap_or(Value::Null),
            );
            payload.insert("z_min".to_owned(), Value::from(baseplate.display.z_min));
            payload.insert("z_max".to_owned(), Value::from(baseplate.display.z_max));
            payload.insert(
                "color".to_owned(),
                Value::from(baseplate.display.color.clone()),
            );
            payload.insert(
                "brightness".to_owned(),
                Value::from(baseplate.display.brightness),
            );
            payload.insert("visible".to_owned(), Value::from(baseplate.display.visible));
            archive_object("baseplate", payload)
        }
    }
}

fn archive_object(kind: &str, payload: Map<String, Value>) -> Value {
    Value::Object(Map::from_iter([
        ("kind".to_owned(), Value::from(kind)),
        ("payload".to_owned(), Value::Object(payload)),
    ]))
}

fn write_gds_sources(entries: &mut Vec<ZipEntry>, objects: &[SceneObject]) -> anyhow::Result<()> {
    let mut seen = HashMap::<String, ()>::new();
    for obj in objects {
        let SceneObject::GdsLayer(layer) = obj else {
            continue;
        };
        if seen.insert(layer.source_key.clone(), ()).is_some() {
            continue;
        }
        let metadata = fs::metadata(&layer.source_path)
            .with_context(|| format!("read GDS metadata: {}", layer.source_path.display()))?;
        if metadata.len() > ZIP_ENTRY_BYTES_MAX as u64 {
            bail!(
                "GDS source is too large to embed: {}",
                layer.source_path.display()
            );
        }
        let data = fs::read(&layer.source_path)
            .with_context(|| format!("read GDS source: {}", layer.source_path.display()))?;
        let entry_name = format!("{RAW_GDS_DIR}/{}", layer.source_key);
        entries.push(ZipEntry::from_bytes(&entry_name, data)?);
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown.gds")
        .to_owned()
}

fn safe_archive_name(name: &str) -> bool {
    !name.contains('\\') && !name.contains("..") && !name.starts_with('/')
}

#[derive(Deserialize)]
struct SceneArchive {
    format_version: u32,
    objects: Vec<SceneArchiveObject>,
}

#[derive(Deserialize)]
struct SceneArchiveObject {
    kind: String,
    payload: Value,
}

#[derive(Clone)]
struct ZipEntry {
    name: String,
    method: u16,
    crc32: u32,
    compressed: Vec<u8>,
    uncompressed_size: u32,
}

impl ZipEntry {
    fn from_json(name: &str, value: &Value) -> anyhow::Result<Self> {
        Self::from_bytes(name, serde_json::to_vec(value)?)
    }

    fn from_bytes(name: &str, data: Vec<u8>) -> anyhow::Result<Self> {
        if !safe_archive_name(name) || name.is_empty() {
            bail!("invalid ZIP entry name: {name}");
        }
        if data.len() > ZIP_ENTRY_BYTES_MAX {
            bail!("ZIP entry too large: {name}");
        }

        let mut crc32 = Crc32Hasher::new();
        crc32.update(&data);
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&data)?;
        Ok(Self {
            name: name.to_owned(),
            method: ZIP_DEFLATE_METHOD,
            crc32: crc32.finalize(),
            compressed: encoder.finish()?,
            uncompressed_size: u32::try_from(data.len())?,
        })
    }
}

fn write_zip_file(path: &Path, entries: &[ZipEntry]) -> anyhow::Result<()> {
    let mut file = fs::File::create(path)
        .with_context(|| format!("create project archive: {}", path.display()))?;
    let mut central_directory = Vec::new();
    let mut offset = 0u64;

    for entry in entries {
        let name = entry.name.as_bytes();
        let name_len = u16::try_from(name.len()).context("ZIP entry name too long")?;
        let compressed_size =
            u32::try_from(entry.compressed.len()).context("ZIP compressed entry too large")?;
        let local_offset = offset;

        write_u32(&mut file, ZIP_LOCAL_HEADER_SIGNATURE)?;
        write_u16(&mut file, ZIP_VERSION_NEEDED)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, entry.method)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 0)?;
        write_u32(&mut file, entry.crc32)?;
        write_u32(&mut file, compressed_size)?;
        write_u32(&mut file, entry.uncompressed_size)?;
        write_u16(&mut file, name_len)?;
        write_u16(&mut file, 0)?;
        file.write_all(name)?;
        file.write_all(&entry.compressed)?;
        offset += 30 + u64::from(name_len) + u64::from(compressed_size);

        write_u32(&mut central_directory, ZIP_CENTRAL_HEADER_SIGNATURE)?;
        write_u16(&mut central_directory, ZIP_VERSION_MADE_BY)?;
        write_u16(&mut central_directory, ZIP_VERSION_NEEDED)?;
        write_u16(&mut central_directory, 0)?;
        write_u16(&mut central_directory, entry.method)?;
        write_u16(&mut central_directory, 0)?;
        write_u16(&mut central_directory, 0)?;
        write_u32(&mut central_directory, entry.crc32)?;
        write_u32(&mut central_directory, compressed_size)?;
        write_u32(&mut central_directory, entry.uncompressed_size)?;
        write_u16(&mut central_directory, name_len)?;
        write_u16(&mut central_directory, 0)?;
        write_u16(&mut central_directory, 0)?;
        write_u16(&mut central_directory, 0)?;
        write_u16(&mut central_directory, 0)?;
        write_u32(&mut central_directory, 0)?;
        write_u32(&mut central_directory, u32::try_from(local_offset)?)?;
        central_directory.extend_from_slice(name);
    }

    let central_dir_offset = offset;
    let central_dir_size = u64::try_from(central_directory.len())?;
    file.write_all(&central_directory)?;
    let entry_count = u16::try_from(entries.len()).context("ZIP entry count too large")?;

    write_u32(&mut file, ZIP_END_SIGNATURE)?;
    write_u16(&mut file, 0)?;
    write_u16(&mut file, 0)?;
    write_u16(&mut file, entry_count)?;
    write_u16(&mut file, entry_count)?;
    write_u32(&mut file, u32::try_from(central_dir_size)?)?;
    write_u32(&mut file, u32::try_from(central_dir_offset)?)?;
    write_u16(&mut file, 0)?;
    Ok(())
}

struct ReadEntry {
    name: String,
    data: Vec<u8>,
}

fn read_zip_file(path: &Path) -> anyhow::Result<Vec<ReadEntry>> {
    let data = fs::read(path)?;
    let end_offset =
        find_end_of_central_directory(&data).ok_or_else(|| anyhow!("ZIP end record not found"))?;
    let central_dir_size = usize::try_from(read_u32(&data, end_offset + 12)?)?;
    let central_dir_offset = usize::try_from(read_u32(&data, end_offset + 16)?)?;
    let central_dir_end = central_dir_offset
        .checked_add(central_dir_size)
        .ok_or_else(|| anyhow!("invalid ZIP central directory size"))?;
    if central_dir_end > data.len() {
        bail!("ZIP central directory is out of bounds");
    }

    let mut entries = Vec::new();
    let mut cursor = central_dir_offset;
    while cursor < central_dir_end {
        if read_u32(&data, cursor)? != ZIP_CENTRAL_HEADER_SIGNATURE {
            bail!("invalid ZIP central directory entry");
        }
        let method = read_u16(&data, cursor + 10)?;
        let compressed_size = usize::try_from(read_u32(&data, cursor + 20)?)?;
        let uncompressed_size = usize::try_from(read_u32(&data, cursor + 24)?)?;
        if uncompressed_size > ZIP_ENTRY_BYTES_MAX {
            bail!("ZIP entry exceeds size limit");
        }
        let name_len = usize::from(read_u16(&data, cursor + 28)?);
        let extra_len = usize::from(read_u16(&data, cursor + 30)?);
        let comment_len = usize::from(read_u16(&data, cursor + 32)?);
        let local_offset = usize::try_from(read_u32(&data, cursor + 42)?)?;
        let name_start = cursor + 46;
        let name_end = checked_range_end(name_start, name_len, data.len())?;
        let name = String::from_utf8(data[name_start..name_end].to_vec())
            .context("ZIP entry name is not UTF-8")?;

        let data_start = local_entry_data_start(&data, local_offset)?;
        let data_end = checked_range_end(data_start, compressed_size, data.len())?;
        let compressed = &data[data_start..data_end];
        let payload = decompress_entry(method, compressed, uncompressed_size)?;
        entries.push(ReadEntry {
            name,
            data: payload,
        });

        cursor = checked_range_end(name_end, extra_len + comment_len, central_dir_end)?;
    }

    Ok(entries)
}

fn local_entry_data_start(data: &[u8], local_offset: usize) -> anyhow::Result<usize> {
    if read_u32(data, local_offset)? != ZIP_LOCAL_HEADER_SIGNATURE {
        bail!("invalid ZIP local header");
    }
    let name_len = usize::from(read_u16(data, local_offset + 26)?);
    let extra_len = usize::from(read_u16(data, local_offset + 28)?);
    checked_range_end(local_offset + 30, name_len + extra_len, data.len())
}

fn decompress_entry(
    method: u16,
    compressed: &[u8],
    uncompressed_size: usize,
) -> anyhow::Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(uncompressed_size);
    match method {
        ZIP_STORE_METHOD => payload.extend_from_slice(compressed),
        ZIP_DEFLATE_METHOD => {
            let mut decoder = DeflateDecoder::new(compressed);
            decoder.read_to_end(&mut payload)?;
        }
        _ => bail!("unsupported ZIP compression method: {method}"),
    }
    if payload.len() != uncompressed_size {
        bail!("ZIP entry size mismatch");
    }
    Ok(payload)
}

fn checked_range_end(start: usize, len: usize, limit: usize) -> anyhow::Result<usize> {
    let end = start
        .checked_add(len)
        .ok_or_else(|| anyhow!("ZIP offset overflow"))?;
    if end > limit {
        bail!("ZIP entry is out of bounds");
    }
    Ok(end)
}

fn find_end_of_central_directory(data: &[u8]) -> Option<usize> {
    if data.len() < 22 {
        return None;
    }
    let start = data.len().saturating_sub(ZIP_SEARCH_BACK_MAX);
    let mut cursor = data.len() - 22;
    loop {
        if read_u32(data, cursor).ok()? == ZIP_END_SIGNATURE {
            return Some(cursor);
        }
        if cursor == start {
            return None;
        }
        cursor -= 1;
    }
}

fn write_u16<W: Write>(writer: &mut W, value: u16) -> anyhow::Result<()> {
    writer.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> anyhow::Result<()> {
    writer.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn read_u16(data: &[u8], offset: usize) -> anyhow::Result<u16> {
    let end = checked_range_end(offset, 2, data.len())?;
    let mut raw = [0u8; 2];
    raw.copy_from_slice(&data[offset..end]);
    Ok(u16::from_le_bytes(raw))
}

fn read_u32(data: &[u8], offset: usize) -> anyhow::Result<u32> {
    let end = checked_range_end(offset, 4, data.len())?;
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&data[offset..end]);
    Ok(u32::from_le_bytes(raw))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::archive::{read_archive, source_key_for_path, write_archive};
    use crate::model::{Bounds2d, DisplayProperties, GdsLayerObject, SceneObject};

    #[test]
    fn source_key_suffix() {
        let key = source_key_for_path(Path::new("models/AWG.gds"));
        assert!(key.starts_with("AWG-"));
        assert!(key.ends_with(".gds"));
    }

    #[test]
    fn archive_round_trip() {
        let temp_dir = std::env::temp_dir().join(format!("gds3d-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let archive_path = temp_dir.join("project.gds3d");
        let gds_path = PathBuf::from("models/AWG.gds");
        let obj = SceneObject::GdsLayer(GdsLayerObject {
            id: "00000000000000000000000000000001".to_owned(),
            display: DisplayProperties::gds_layer("L4/1"),
            file_path: gds_path.clone(),
            source_path: gds_path.clone(),
            source_key: source_key_for_path(&gds_path),
            cell_name: "AWG".to_owned(),
            layer: 4,
            datatype: 1,
            bounds: Bounds2d {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 1.0,
                max_y: 1.0,
            },
            polygons: Vec::new(),
        });

        write_archive(&archive_path, &[obj]).expect("write archive");
        let (objects, sources) = read_archive(&archive_path).expect("read archive");
        assert_eq!(objects.len(), 1);
        let payload = objects[0].payload.as_object().expect("object payload");
        assert_eq!(
            payload.get("object_id").and_then(serde_json::Value::as_str),
            Some("00000000000000000000000000000001")
        );
        assert_eq!(
            payload
                .get("display_path")
                .and_then(serde_json::Value::as_str),
            Some("models/AWG.gds")
        );
        assert_eq!(sources.len(), 1);
        assert!(sources.values().next().is_some_and(|data| !data.is_empty()));
        fs::remove_dir_all(&temp_dir).expect("remove temp dir");
    }
}
