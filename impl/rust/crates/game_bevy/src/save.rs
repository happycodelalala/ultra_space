//! Saving and loading (task 3.13; design: vehicle-builder.md §6): F5 quick saves, F9 quick
//! loads. The simulation's part of a save is `sim_vehicle::save`; this adds the player's (the
//! ship they fly, whether they are on foot, the view) and reconnects the game to the loaded
//! world (terrain tiles, render states).
//!
//! Saves are `<saves>/<name>.json`: `ULTRA_SPACE_SAVES` if set, else `saves/` next to the
//! executable.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sim_core::ObjectId;
use sim_vehicle::save::{SaveFile, capture, restore};

use crate::ship::{PilotState, PlayerShip, ShipRenderState, ShipView};
use crate::sim::Sim;
use crate::terrain::{Terrains, replace_terrain};
use crate::universe::{Catalog, InGame, Universe};
use crate::walk::{OnFoot, WalkerRenderState};

/// The quick save's name.
const QUICK: &str = "quick";

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, quick_save_load.in_set(InGame));
    }
}

/// The folder saves go in.
pub fn saves_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ULTRA_SPACE_SAVES") {
        return PathBuf::from(dir);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("saves")))
        .unwrap_or_else(|| PathBuf::from("saves"))
}

/// The player's part of a save.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct PlayerRecord {
    /// The vehicle the player flies (or last flew, when on foot).
    ship: Option<u64>,
    /// The walker, when on foot, and the look pitch (radians).
    on_foot: Option<(u64, f64)>,
    /// "chase" or "cockpit".
    view: String,
    /// Chase-camera orbit (yaw, pitch), radians.
    orbit: (f64, f64),
}

#[allow(clippy::too_many_arguments)]
fn quick_save_load(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    universe: Res<Universe>,
    catalog: Res<Catalog>,
    ship: Option<Res<PlayerShip>>,
    on_foot: Option<Res<OnFoot>>,
    mut pilot: ResMut<PilotState>,
    mut sim: ResMut<Sim>,
    mut terrains: ResMut<Terrains>,
    mut tracked: ResMut<ShipRenderState>,
    mut walker_render: ResMut<WalkerRenderState>,
    mut notice: ResMut<crate::hud::Notice>,
    real: Res<Time<Real>>,
) {
    let save = keys.just_pressed(KeyCode::F5);
    let load = keys.just_pressed(KeyCode::F9);
    if !save && !load {
        return;
    }
    let now = real.elapsed_secs_f64();
    if pilot.building {
        notice.show("Leave the builder (B) to save or load", now);
        return;
    }
    let path = saves_dir().join(format!("{QUICK}.json"));
    let system = &universe.0;
    if save {
        let mut file = capture(&sim.0, system);
        let player = PlayerRecord {
            ship: ship.map(|s| s.id.0),
            on_foot: on_foot.map(|f| (f.id.0, f.pitch)),
            view: match pilot.view {
                ShipView::Chase => "chase",
                ShipView::Cockpit => "cockpit",
            }
            .into(),
            orbit: pilot.orbit,
        };
        file.player = serde_json::to_value(player).expect("player record");
        let text = match write_save(&path, &file) {
            Ok(()) => {
                info!("saved {} objects to {}", file.objects.len(), path.display());
                "Quick saved (F9 loads)".to_string()
            }
            Err(e) => {
                warn!("save to {} failed: {e}", path.display());
                format!("Save failed: {e}")
            }
        };
        notice.show(text, now);
        return;
    }

    let file = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| SaveFile::from_json(&text).map_err(|e| e.to_string()))
    {
        Ok(f) => f,
        Err(e) => {
            warn!("load from {} failed: {e}", path.display());
            notice.show(format!("No quick save to load ({e})"), now);
            return;
        }
    };
    let report = restore(&mut sim.0, system, &catalog.0, &file);
    for p in &report.problems {
        warn!("load: {p}");
    }
    // The renderer's tiles follow the loaded terrain.
    for &body in &report.terrain_changed {
        if let Some(terrain) = sim
            .0
            .regions
            .get(body)
            .and_then(|r| r.world.get_resource::<sim_vehicle::flight::RegionTerrain>())
            .and_then(|t| t.0.clone())
        {
            replace_terrain(&mut terrains, &mut commands, body, terrain);
        }
    }
    // The player takes up where they were.
    let player: PlayerRecord = serde_json::from_value(file.player.clone()).unwrap_or_default();
    let exists = |id: u64| sim.0.objects.location(ObjectId(id)).is_some();
    let ship_id = player
        .ship
        .filter(|&id| exists(id))
        .or_else(|| first_vehicle(&sim.0));
    if let Some(id) = ship_id {
        commands.insert_resource(PlayerShip { id: ObjectId(id) });
    }
    match player.on_foot.filter(|&(id, _)| exists(id)) {
        Some((id, pitch)) => {
            commands.insert_resource(OnFoot {
                id: ObjectId(id),
                pitch,
            });
            pilot.on_foot = true;
        }
        None => {
            commands.remove_resource::<OnFoot>();
            pilot.on_foot = false;
        }
    }
    pilot.view = if player.view == "cockpit" {
        ShipView::Cockpit
    } else {
        ShipView::Chase
    };
    pilot.orbit = player.orbit;
    *tracked = ShipRenderState::default();
    *walker_render = WalkerRenderState::default();
    info!(
        "loaded {} objects from {} ({} problems)",
        file.objects.len(),
        path.display(),
        report.problems.len()
    );
    notice.show(
        if report.problems.is_empty() {
            "Quick loaded".to_string()
        } else {
            format!(
                "Quick loaded; {} things no longer exist (see the log)",
                report.problems.len()
            )
        },
        now,
    );
}

/// The lowest-numbered vehicle (the player's ship when the save names none that still exists).
fn first_vehicle(sim: &sim_core::Simulation) -> Option<u64> {
    let mut ids: Vec<ObjectId> = sim.objects.ids().collect();
    ids.sort();
    ids.into_iter().map(|id| id.0).find(|&id| {
        sim.objects.location(ObjectId(id)).is_some_and(|loc| {
            sim.regions
                .get(loc.region)
                .is_some_and(|r| r.world.get::<sim_vehicle::Vehicle>(loc.entity).is_some())
        })
    })
}

/// Writes `file` to `path` through a temporary file, so a crash never leaves half a save.
fn write_save(path: &std::path::Path, file: &SaveFile) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, file.to_json())?;
    std::fs::rename(&tmp, path)
}
