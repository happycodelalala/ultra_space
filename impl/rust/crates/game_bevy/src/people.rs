//! People on screen (task 4.7; design: living-planet.md §6): each culture's characters
//! (`assets/export/characters/<clothing set>/`, one glTF binary per character with the shared
//! animations), loaded through Bevy's glTF loader, skinned and animated.
//!
//! When a settlement's buildings are shown, some of its people are promoted (task 4.8,
//! `sim_society::people`): a crowd that lives out the day by the settlement's local time, each
//! person drawn as a character of their culture who takes their role, walking the streets,
//! working, gathering, hidden indoors; demoted when the buildings go.

use std::collections::HashMap;

use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady};
use glam::DVec2;

use sim_core::time::{UniverseTime, local_solar_fraction};
use sim_society::{Activity, Crowd};

use crate::camera::SpaceCamera;
use crate::settlements::SettlementDraw;
use crate::sim::RenderTime;
use crate::universe::{GameState, InGame, Universe, assets_root};

pub struct PeoplePlugin;

impl Plugin for PeoplePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Running), load_characters)
            .add_systems(
                Update,
                (prepare_characters, stream_people, update_people)
                    .chain()
                    .after(crate::settlements::SettlementSet)
                    .in_set(InGame),
            );
    }
}

/// A character ready to spawn: its scene, its animation graph and the graph node of each
/// animation by name.
#[derive(Clone)]
struct Ready {
    scene: Handle<WorldAsset>,
    graph: Handle<AnimationGraph>,
    nodes: HashMap<String, AnimationNodeIndex>,
}

struct Kind {
    id: String,
    roles: Vec<String>,
    gltf: Handle<Gltf>,
    ready: Option<Ready>,
}

struct CharacterSet {
    animations: Vec<String>,
    kinds: Vec<Kind>,
}

#[derive(Resource, Default)]
struct Characters {
    sets: HashMap<String, CharacterSet>,
    /// Promoted people per settlement (index into the drawn sites).
    shown: HashMap<usize, Shown>,
}

/// A promoted person on screen: their place in the crowd, their animations and the one
/// playing.
#[derive(Component)]
struct Resident {
    index: usize,
    graph: Handle<AnimationGraph>,
    nodes: HashMap<String, AnimationNodeIndex>,
    playing: Option<String>,
    /// The entity with their animation player, once their scene is spawned.
    player: Option<Entity>,
    offset: f32,
}

/// What looking at a person shows: name, role (the culture's word) and culture.
#[derive(Component)]
pub(crate) struct PersonLabel(pub String);

/// A settlement's promoted people and their entities.
struct Shown {
    crowd: Crowd,
    entities: Vec<Entity>,
}

fn load_characters(mut commands: Commands, assets: Res<AssetServer>) {
    let mut characters = Characters::default();
    let Some(root) = assets_root() else {
        warn!("characters not found: settlements have no people");
        commands.insert_resource(characters);
        return;
    };
    let Ok(entries) = std::fs::read_dir(root.join("characters")) else {
        commands.insert_resource(characters);
        return;
    };
    for entry in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path().join("set.json")) else {
            continue;
        };
        let Ok(set) = serde_json::from_str::<serde_json::Value>(&text) else {
            warn!("characters: bad {}", entry.path().display());
            continue;
        };
        let id = set["id"].as_str().unwrap_or_default().to_string();
        let strings = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let kinds = set["characters"]
            .as_array()
            .map(|cs| {
                cs.iter()
                    .map(|c| Kind {
                        id: c["id"].as_str().unwrap_or_default().to_string(),
                        roles: strings(&c["roles"]),
                        gltf: assets.load(format!(
                            "characters/{id}/{}",
                            c["model"].as_str().unwrap_or_default()
                        )),
                        ready: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        characters.sets.insert(
            id,
            CharacterSet {
                animations: strings(&set["animations"]),
                kinds,
            },
        );
    }
    info!(
        "characters: {} sets, {} characters",
        characters.sets.len(),
        characters
            .sets
            .values()
            .map(|s| s.kinds.len())
            .sum::<usize>()
    );
    commands.insert_resource(characters);
}

/// Builds each loaded character's animation graph.
fn prepare_characters(
    mut commands: Commands,
    characters: Option<ResMut<Characters>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    warm: Option<Res<crate::warmup::WarmupDone>>,
    cameras: Query<Entity, With<crate::space_render::MainCamera>>,
) {
    let Some(mut characters) = characters else {
        return;
    };
    for set in characters.sets.values_mut() {
        for kind in set.kinds.iter_mut().filter(|k| k.ready.is_none()) {
            let Some(gltf) = gltfs.get(&kind.gltf) else {
                continue;
            };
            let Some(scene) = gltf
                .default_scene
                .clone()
                .or_else(|| gltf.scenes.first().cloned())
            else {
                continue;
            };
            let named: Vec<(String, Handle<AnimationClip>)> = set
                .animations
                .iter()
                .filter_map(|name| {
                    Some((
                        name.clone(),
                        gltf.named_animations.get(name.as_str())?.clone(),
                    ))
                })
                .collect();
            if named.len() < set.animations.len() {
                warn!("character {}: missing animations", kind.id);
            }
            let (graph, indices) = AnimationGraph::from_clips(named.iter().map(|n| n.1.clone()));
            // Its pipelines (skinned meshes, its textures) compiled during the warm-up.
            if warm.is_none() {
                crate::warmup::spawn_primer(
                    &mut commands,
                    cameras.iter().next(),
                    WorldAssetRoot(scene.clone()),
                );
            }
            kind.ready = Some(Ready {
                scene,
                graph: graphs.add(graph),
                nodes: named.into_iter().map(|n| n.0).zip(indices).collect(),
            });
        }
    }
}

/// A small deterministic hash for choices (which character plays whom).
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// How many of a settlement's people are promoted: one in twenty-five, 8 to 64.
fn promoted(population: u32) -> usize {
    (population as usize / 25).clamp(8, sim_society::people::MAX_PEOPLE)
}

/// The animation for what a person is doing (the work animation by role).
fn animation(activity: Activity, role: &str) -> &'static str {
    match activity {
        Activity::Walking => "walk",
        Activity::Working => match role {
            "smith" | "carpenter" | "technician" | "engineer" => "use",
            "laborer" | "fisher" => "carry",
            "merchant" | "trader" | "guard" | "security" | "pilot" => "talk",
            _ => "work",
        },
        Activity::Talking => "talk",
        Activity::Idle | Activity::Inside => "idle",
    }
}

/// Local solar time at a settlement (fraction of its day, 0 midnight) at universe time `t`.
fn local_day(universe: &Universe, site: &crate::settlements::Site, t: f64) -> f64 {
    let system = &universe.0;
    let c = site.settlement.direction;
    local_solar_fraction(
        &system.frames,
        system.bodies[site.body].surface_frame,
        system.star().inertial_frame,
        (-c.z).atan2(c.x),
        UniverseTime::from_nanos((t.max(0.0) * 1e9) as u64),
    )
}

/// Promotes people where a settlement's buildings are shown, demotes them when they go.
fn stream_people(
    mut commands: Commands,
    characters: Option<ResMut<Characters>>,
    draw: Option<Res<SettlementDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
) {
    let _slow = crate::debug_overlay::SlowSystem::new("stream_people");
    let (Some(mut characters), Some(draw)) = (characters, draw) else {
        return;
    };
    let characters = &mut *characters;
    for (i, site) in draw.sites.iter().enumerate() {
        match (site.near.is_some(), characters.shown.contains_key(&i)) {
            (true, false) => {
                let Some(set) = characters.sets.get(&site.clothing) else {
                    continue;
                };
                if set.kinds.iter().any(|k| k.ready.is_none()) {
                    continue; // still loading
                }
                let Some(layout) = site.layout.clone() else {
                    continue;
                };
                let Some(kit) = draw.kits.0.get(&layout.kit) else {
                    continue;
                };
                let mut crowd = Crowd::promote(
                    &site.settlement,
                    &layout,
                    kit,
                    &site.naming,
                    promoted(site.settlement.population),
                );
                crowd.settle(local_day(&universe, site, render.0));
                let mut entities = Vec::with_capacity(crowd.people.len());
                for (k, person) in crowd.people.iter().enumerate() {
                    let h = mix(site.seed ^ (k as u64 + 1).wrapping_mul(0x9e37_79b9));
                    // A character who takes the role (any, if none does).
                    let fit: Vec<&Kind> = set
                        .kinds
                        .iter()
                        .filter(|c| c.roles.contains(&person.role))
                        .collect();
                    let kind = if fit.is_empty() {
                        &set.kinds[(h >> 8) as usize % set.kinds.len()]
                    } else {
                        fit[(h >> 8) as usize % fit.len()]
                    };
                    let Some(ready) = &kind.ready else { continue };
                    let e = commands
                        .spawn((
                            Name::new(format!("{} ({})", person.name, person.role)),
                            PersonLabel(format!(
                                "{}, {} ({})",
                                person.name,
                                site.role_names.get(&person.role).unwrap_or(&person.role),
                                site.culture.1
                            )),
                            WorldAssetRoot(ready.scene.clone()),
                            Transform::default(),
                            Visibility::Hidden,
                            Resident {
                                index: k,
                                graph: ready.graph.clone(),
                                nodes: ready.nodes.clone(),
                                playing: None,
                                player: None,
                                offset: ((h >> 40) % 1000) as f32 / 1000.0 * 2.0,
                            },
                            ChildOf(site.root),
                        ))
                        .observe(found_animator)
                        .id();
                    entities.push(e);
                }
                characters.shown.insert(i, Shown { crowd, entities });
            }
            (false, true) => {
                if let Some(shown) = characters.shown.remove(&i) {
                    for e in shown.entities {
                        commands.entity(e).despawn();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Steps every promoted crowd by the settlement's local time and moves its people: on the
/// ground, facing where they go, hidden indoors, animated for what they do.
#[allow(clippy::too_many_arguments)]
fn update_people(
    mut commands: Commands,
    characters: Option<ResMut<Characters>>,
    draw: Option<Res<SettlementDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    time: Res<Time>,
    mut residents: Query<(&mut Resident, &mut Transform, &mut Visibility)>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let _slow = crate::debug_overlay::SlowSystem::new("update_people");
    let (Some(mut characters), Some(draw)) = (characters, draw) else {
        return;
    };
    let dt = f64::from(time.delta_secs()).min(0.25);
    for (i, shown) in characters.shown.iter_mut() {
        let site = &draw.sites[*i];
        let (Some(layout), Some(terrain)) = (site.layout.clone(), draw.terrain(site.body).cloned())
        else {
            continue;
        };
        let r = terrain.radius;
        let day = local_day(&universe, site, render.0);
        // The player: the camera, on the settlement's plane, if within a few hundred metres.
        let cam = crate::settlements::camera_on(&universe, &camera, site.body, render.0);
        let off = site.basis.inverse() * (cam - site.origin);
        let player = (off.length() < 400.0).then(|| DVec2::new(off.x, off.z));
        shown.crowd.step(dt, day, player);
        for &e in &shown.entities {
            let Ok((mut resident, mut transform, mut visibility)) = residents.get_mut(e) else {
                continue;
            };
            let Some(person) = shown.crowd.people.get(resident.index) else {
                continue;
            };
            let want = if person.activity == Activity::Inside {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            if *visibility != want {
                *visibility = want;
            }
            if want == Visibility::Hidden {
                continue;
            }
            let p = person.position;
            let up = layout.direction(p.x, p.y, r);
            let ground = terrain.surface_height(up, 1.0);
            transform.translation =
                (site.basis.inverse() * (up * (r + ground) - site.origin)).as_vec3();
            transform.rotation = Quat::from_rotation_y(person.heading as f32);
            // The animation for what they do now.
            let name = animation(person.activity, &person.role);
            if resident.playing.as_deref() != Some(name)
                && let Some(animator) = resident.player
                && let Ok(mut player) = players.get_mut(animator)
                && let Some(&node) = resident
                    .nodes
                    .get(name)
                    .or_else(|| resident.nodes.get("idle"))
            {
                player.stop_all();
                player.play(node).repeat().seek_to(resident.offset);
                resident.playing = Some(name.to_string());
                commands
                    .entity(animator)
                    .insert(AnimationGraphHandle(resident.graph.clone()));
            }
        }
    }
}

/// Finds a person's animation player once its scene is spawned (on the skeleton's root,
/// somewhere below the person); the update starts the right animation.
fn found_animator(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    mut residents: Query<&mut Resident>,
    players: Query<(), With<AnimationPlayer>>,
) {
    let Ok(mut resident) = residents.get_mut(ready.entity) else {
        return;
    };
    for child in children.iter_descendants(ready.entity) {
        if players.contains(child) {
            resident.player = Some(child);
            resident.playing = None;
            commands
                .entity(child)
                .insert(AnimationGraphHandle(resident.graph.clone()));
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn every_character_has_a_skin_and_the_shared_animations() {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../assets/export/characters");
        let mut count = 0;
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let set: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(entry.path().join("set.json")).unwrap(),
            )
            .unwrap();
            let wanted: Vec<&str> = set["animations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap())
                .collect();
            assert!(wanted.contains(&"walk") && wanted.contains(&"idle"));
            for c in set["characters"].as_array().unwrap() {
                let model = entry.path().join(c["model"].as_str().unwrap());
                let gltf = gltf::Gltf::open(&model).unwrap();
                let names: Vec<&str> = gltf.animations().filter_map(|a| a.name()).collect();
                for w in &wanted {
                    assert!(
                        names.contains(w),
                        "{}: no {w} in {names:?}",
                        model.display()
                    );
                }
                assert_eq!(gltf.skins().count(), 1, "{}", model.display());
                assert!(!c["roles"].as_array().unwrap().is_empty());
                count += 1;
            }
        }
        assert!(count >= 6, "{count} characters");
    }
}
