//! Part models (task 3.12): low-poly glTF binaries in `assets/export/models/<model>.glb` (the
//! repo) or `assets/models/` (next to an exported executable), named by a part form's `model`.
//! Each primitive's material name picks the game material (a slot of
//! `content/data/part_materials.json`); its COLOR_0 is baked ambient occlusion.
//! Loaded once, on first use, on whichever thread asks (the mesh workers); parts without a
//! model keep their placeholder shape.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::vehicle_visual::PartMaterial;

/// A model: one mesh per material, in part axes.
pub type PartModel = Vec<(PartMaterial, Mesh)>;

/// The models folder: `ULTRA_SPACE_MODELS`, else `assets/models` (exported) or
/// `assets/export/models` (repo) next to the executable or the working directory or any of
/// their parents.
pub fn models_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ULTRA_SPACE_MODELS") {
        return Some(PathBuf::from(dir));
    }
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    starts.into_iter().flatten().find_map(|start| {
        start.ancestors().find_map(|dir| {
            ["assets/models", "assets/export/models"]
                .iter()
                .map(|sub| dir.join(sub))
                .find(|d| d.is_dir())
        })
    })
}

/// Every model, by name (empty without a models folder).
pub fn part_models() -> &'static HashMap<String, PartModel> {
    static MODELS: OnceLock<HashMap<String, PartModel>> = OnceLock::new();
    MODELS.get_or_init(|| {
        let Some(dir) = models_dir() else {
            info!("part models: no models folder, parts drawn as plain shapes");
            return HashMap::new();
        };
        let models = load_dir(&dir);
        info!("part models: {} from {}", models.len(), dir.display());
        models
    })
}

fn load_dir(dir: &Path) -> HashMap<String, PartModel> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    let mut models = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "glb") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| parse_glb(&b))
        {
            Ok(model) => {
                models.insert(name.to_string(), model);
            }
            Err(e) => warn!("part model {}: {e}", path.display()),
        }
    }
    models
}

/// Reads a glTF binary's first mesh: positions, normals and indices of each primitive.
pub fn parse_glb(bytes: &[u8]) -> Result<PartModel, String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
    let blob = gltf.blob.as_deref();
    let mesh = gltf.meshes().next().ok_or("no mesh")?;
    let mut out: PartModel = Vec::new();
    for primitive in mesh.primitives() {
        let reader = primitive.reader(|_| blob);
        let positions: Vec<[f32; 3]> = reader.read_positions().ok_or("no positions")?.collect();
        let normals: Vec<[f32; 3]> = reader.read_normals().ok_or("no normals")?.collect();
        let indices: Vec<u32> = reader
            .read_indices()
            .ok_or("no indices")?
            .into_u32()
            .collect();
        let uvs = vec![[0.0f32, 0.0]; positions.len()];
        // Baked ambient occlusion (white without it).
        let colors: Vec<[f32; 4]> = match reader.read_colors(0) {
            Some(c) => c.into_rgba_f32().collect(),
            None => vec![[1.0; 4]; positions.len()],
        };
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices));
        let material = PartMaterial::named(primitive.material().name());
        match out.iter_mut().find(|(m, _)| *m == material) {
            Some((_, into)) => into.merge(&mesh).map_err(|e| e.to_string())?,
            None => out.push((material, mesh)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generated_models_load() {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../assets/export/models");
        let models = load_dir(&dir);
        assert!(models.len() >= 15, "{} models", models.len());
        let torso = &models["frame.torso"];
        assert!(torso.iter().any(|(m, _)| *m == PartMaterial::Trim));
        // Sized to the part: the torso is 5 x 5 x 3.5 m (details such as the hatch, ladder and
        // radiator fins stand out from it by up to a tenth).
        for (_, mesh) in torso {
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions");
            };
            for p in points {
                assert!(
                    p[0].abs() <= 2.75 && p[1].abs() <= 2.75 && p[2].abs() <= 1.93,
                    "{p:?} near the 5 x 5 x 3.5 m part"
                );
            }
        }
    }
}
