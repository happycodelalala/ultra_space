//! The exported block models (tools/blocks) mesh here to the same surface as in the authoring
//! scripts: the visible area of every colour matches the scripts' own mesher.

use std::path::PathBuf;

use sim_blocks::BlockModel;

fn blocks_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../assets/export/blocks")
}

#[test]
fn every_model_matches_its_reference() {
    let mut checked = 0;
    for entry in std::fs::read_dir(blocks_dir()).expect("assets/export/blocks exists") {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        if path.extension().is_none_or(|e| e != "json") || stem == "palette" || stem == "village" {
            continue;
        }
        let model = BlockModel::load(&path).unwrap_or_else(|e| panic!("{e}"));
        for (n, part) in model.parts.iter().enumerate() {
            let m = part.mesh();
            let reference = part.reference.as_ref().expect("reference areas");
            let got = m.area_by_colour();
            for (name, &want) in &reference.area {
                let index = part.palette.iter().position(|p| p == name).unwrap() as u16 + 1;
                let have = got.get(&index).copied().unwrap_or(0.0);
                assert!(
                    (have - want).abs() <= 1e-4 + want * 1e-4,
                    "{} part {n} colour {name}: area {have:.5} m² here, {want:.5} m² in the scripts",
                    model.name
                );
            }
            assert_eq!(
                got.len(),
                reference.area.len(),
                "{} part {n}: colours drawn",
                model.name
            );
        }
        checked += 1;
    }
    assert!(checked > 50, "only {checked} models found");
}
