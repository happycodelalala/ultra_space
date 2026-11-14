//! People on screen (task 4.7; design: living-planet.md §6): each culture's characters
//! (`assets/export/characters/<clothing set>/`, one glTF binary per character with the shared
//! animations), loaded through Bevy's glTF loader, skinned and animated.
//!
//! When a settlement's buildings are shown, some of its people appear on its streets near the
//! centre: characters chosen by its people's roles, each idling, working, talking or walking in
//! place. (Schedules and walking the streets come with task 4.8.)

use std::collections::HashMap;

use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady};
use glam::DVec2;

use crate::settlements::{SettlementDraw, Site};
use crate::universe::{GameState, InGame, assets_root};

/// People shown in a settlement whose buildings are shown.
const PEOPLE_PER_SETTLEMENT: usize = 10;
/// What they do, in turn.
const ACTIVITIES: [&str; 10] = [
    "idle", "work", "talk", "walk", "use", "idle", "carry", "talk", "work", "walk",
];

pub struct PeoplePlugin;

impl Plugin for PeoplePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Running), load_characters)
            .add_systems(
                Update,
                (prepare_characters, stream_people)
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
    /// People spawned per settlement (index into the drawn sites).
    shown: HashMap<usize, Vec<Entity>>,
}

/// A person's animation, started when its scene is ready.
#[derive(Component)]
struct Person {
    graph: Handle<AnimationGraph>,
    node: AnimationNodeIndex,
    offset: f32,
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
    characters: Option<ResMut<Characters>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
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
            kind.ready = Some(Ready {
                scene,
                graph: graphs.add(graph),
                nodes: named.into_iter().map(|n| n.0).zip(indices).collect(),
            });
        }
    }
}

/// A small deterministic hash for choices (who stands where, doing what).
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Spots on the settlement's streets nearest its centre, a few metres apart (tangent plane).
fn spots(site: &Site, n: usize) -> Vec<DVec2> {
    let l = &site.layout;
    let mut nodes: Vec<DVec2> = l
        .streets
        .iter()
        .flat_map(|&(a, b)| {
            let (a, b) = (l.nodes[a as usize], l.nodes[b as usize]);
            [a, a.lerp(b, 0.5)]
        })
        .collect();
    nodes.sort_by(|a, b| a.length().total_cmp(&b.length()));
    let mut out: Vec<DVec2> = Vec::new();
    for p in nodes {
        if out.len() == n {
            break;
        }
        if out.iter().all(|q| q.distance(p) > 4.0) {
            out.push(p);
        }
    }
    out
}

/// Shows people in the settlements whose buildings are shown.
fn stream_people(
    mut commands: Commands,
    characters: Option<ResMut<Characters>>,
    draw: Option<Res<SettlementDraw>>,
) {
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
                let total: u32 = site.roles.iter().map(|r| r.1).sum();
                let r = site.terrain.radius;
                let mut people = Vec::new();
                for (k, spot) in spots(site, PEOPLE_PER_SETTLEMENT).into_iter().enumerate() {
                    let h = mix(site.seed ^ (k as u64 + 1).wrapping_mul(0x9e37_79b9));
                    // A role by the settlement's shares, then a character who takes it.
                    let mut pick = (h % u64::from(total.max(1))) as u32;
                    let role = site
                        .roles
                        .iter()
                        .find(|(_, n)| {
                            if pick < *n {
                                true
                            } else {
                                pick -= n;
                                false
                            }
                        })
                        .map_or("", |r| r.0.as_str());
                    let fit: Vec<&Kind> = set
                        .kinds
                        .iter()
                        .filter(|c| c.roles.iter().any(|x| x == role))
                        .collect();
                    let kind = if fit.is_empty() {
                        &set.kinds[(h >> 8) as usize % set.kinds.len()]
                    } else {
                        fit[(h >> 8) as usize % fit.len()]
                    };
                    let Some(ready) = &kind.ready else { continue };
                    let activity = ACTIVITIES[k % ACTIVITIES.len()];
                    let Some(&node) = ready
                        .nodes
                        .get(activity)
                        .or_else(|| ready.nodes.get("idle"))
                    else {
                        continue;
                    };
                    // Off the street's centre line a little, on the ground.
                    let side = DVec2::new(
                        ((h >> 16) % 100) as f64 / 50.0 - 1.0,
                        ((h >> 24) % 100) as f64 / 50.0 - 1.0,
                    );
                    let at = spot + side * 1.5;
                    let up = site.layout.direction(at.x, at.y, r);
                    let ground = site.terrain.surface_height(up, 1.0);
                    let local = site.basis.inverse() * (up * (r + ground) - site.origin);
                    let yaw = ((h >> 32) % 360) as f32 * std::f32::consts::PI / 180.0;
                    let e = commands
                        .spawn((
                            Name::new(format!("person ({role})")),
                            WorldAssetRoot(ready.scene.clone()),
                            Transform {
                                translation: local.as_vec3(),
                                rotation: Quat::from_rotation_y(yaw),
                                ..default()
                            },
                            Person {
                                graph: ready.graph.clone(),
                                node,
                                offset: ((h >> 40) % 1000) as f32 / 1000.0 * 2.0,
                            },
                            ChildOf(site.root),
                        ))
                        .observe(start_animation)
                        .id();
                    people.push(e);
                }
                characters.shown.insert(i, people);
            }
            (false, true) => {
                for e in characters.shown.remove(&i).unwrap_or_default() {
                    commands.entity(e).despawn();
                }
            }
            _ => {}
        }
    }
}

/// Starts a person's animation once its scene is spawned (the animation player is on the
/// skeleton's root, somewhere below the person).
fn start_animation(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    people: Query<&Person>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let Ok(person) = people.get(ready.entity) else {
        return;
    };
    for child in children.iter_descendants(ready.entity) {
        if let Ok(mut player) = players.get_mut(child) {
            player.play(person.node).repeat().seek_to(person.offset);
            commands
                .entity(child)
                .insert(AnimationGraphHandle(person.graph.clone()));
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
