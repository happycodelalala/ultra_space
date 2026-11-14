//! Terrain collision for physics (Batch 3, task 3.2; design: vehicle-builder.md §2.2).
//!
//! The ground near physics bodies is a set of **collision patches**: triangle meshes built from
//! the terrain function (deltas included) on the fixed-level cube-sphere lattice, so physics
//! stands on exactly the heights the renderer draws and neighbouring patches share vertices.
//! The patches hang on one kinematic body that turns with the planet, so a contact's ground
//! velocity is `spin × r` and landed vehicles ride along through friction.
//!
//! Patches are built in the background (a few milliseconds each), around every body near the
//! ground and ahead along its motion. Where a body's patches aren't ready yet, the physics scene
//! falls back to the Batch 2 contact model, so nothing falls through.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};

use glam::{DQuat, DVec3};
use rapier3d_f64::prelude::{
    ColliderBuilder, ColliderHandle, Group, InteractionGroups, InteractionTestMode, PhysicsWorld,
    Pose, RigidBodyBuilder, RigidBodyHandle, SharedShape,
};
use sim_planet::quadtree::level_for_spacing;
use sim_planet::{Terrain, TileKey, collision_patch, locate};

/// Collision group of ground patches (resting bodies leave it out of their filter).
pub(crate) const GROUND_GROUP: Group = Group::GROUP_1;

/// Target vertex spacing of collision patches (m).
const PATCH_SPACING: f64 = 2.0;
/// Finest terrain wavelength in patches (m), as for the Batch 2 ground contact.
const PATCH_DETAIL: f64 = 1.0;
/// Ground friction coefficient (combined with the other collider's by averaging).
const GROUND_FRICTION: f64 = 0.8;
/// Patches no body has wanted for this long are dropped (s).
const PATCH_LIFETIME: f64 = 2.0;
/// Most patches kept: they all turn with the ground body, so rapier moves every one of them
/// every step (dozens cost milliseconds); the least recently wanted go first.
const MAX_PATCHES: usize = 64;
/// Background threads building patches.
const WORKERS: usize = 2;
/// Most finished patches added to the physics world per tick.
const PATCHES_PER_TICK: usize = 8;

type Job = Box<dyn FnOnce() + Send>;

/// The shared pool that builds patches.
fn workers() -> &'static Mutex<Sender<Job>> {
    static POOL: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    POOL.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for i in 0..WORKERS {
            let rx = Arc::clone(&rx);
            std::thread::Builder::new()
                .name(format!("collision-patches-{i}"))
                .spawn(move || {
                    loop {
                        let job = match rx.lock() {
                            Ok(rx) => rx.recv(),
                            Err(_) => return,
                        };
                        match job {
                            Ok(job) => job(),
                            Err(_) => return,
                        }
                    }
                })
                .expect("spawn collision patch worker");
        }
        Mutex::new(tx)
    })
}

struct Patch {
    collider: ColliderHandle,
    wanted_at: f64,
}

/// A region's ground: the kinematic body turning with the planet and its collision patches.
pub(crate) struct Ground {
    terrain: Arc<Terrain>,
    level: u8,
    /// Edge length of a patch (m).
    patch_size: f64,
    pub(crate) body: RigidBodyHandle,
    patches: HashMap<TileKey, Patch>,
    /// Buildings' collision boxes near bodies, by (site, building).
    boxes: HashMap<(u32, u32), Patch>,
    pending: HashSet<TileKey>,
    /// Patches finished by the workers (with the terrain generation they were built from),
    /// waiting to be added.
    done: Arc<Mutex<Vec<(u64, TileKey, SharedShape)>>>,
    /// Bumped whenever the terrain changes; older results are dropped.
    generation: u64,
    /// Build patches on the calling thread (tests).
    inline: bool,
}

impl Ground {
    pub(crate) fn new(world: &mut PhysicsWorld, terrain: Arc<Terrain>, inline: bool) -> Self {
        let level = level_for_spacing(terrain.radius, PATCH_SPACING);
        let key = locate(DVec3::X, level);
        let patch_size = key.edge_length(terrain.radius);
        // Velocity-based: it turns at the planet's spin (and its velocity can be read and set
        // like any body's; position-based kinematic bodies ignore velocity setters).
        let body = world.insert_body(RigidBodyBuilder::kinematic_velocity_based().build());
        Self {
            terrain,
            level,
            patch_size,
            body,
            patches: HashMap::new(),
            boxes: HashMap::new(),
            pending: HashSet::new(),
            done: Arc::default(),
            generation: 0,
            inline,
        }
    }

    pub(crate) fn terrain(&self) -> &Arc<Terrain> {
        &self.terrain
    }

    /// Switches to `terrain`, the same planet with more deltas (pads) added: only the patches the
    /// new deltas touch are rebuilt, and each old patch stays until its replacement is ready, so
    /// bodies standing on it don't notice. Returns `false` (nothing changed) when `terrain` isn't
    /// an extension of the current one.
    pub(crate) fn extend_terrain(&mut self, terrain: Arc<Terrain>) -> bool {
        let old = self.terrain.pads();
        let new = terrain.pads();
        if terrain.radius != self.terrain.radius
            || new.len() <= old.len()
            || new[..old.len()] != *old
        {
            return false;
        }
        let added: Vec<_> = new[old.len()..].to_vec();
        self.terrain = terrain;
        self.generation += 1;
        self.pending.clear();
        let spread = self.patch_size / self.terrain.radius;
        let touched: Vec<TileKey> = self
            .patches
            .keys()
            .filter(|k| {
                added
                    .iter()
                    .any(|p| self.terrain.pad_touches(p, k.center(), spread))
            })
            .copied()
            .collect();
        for key in touched {
            self.request(key);
        }
        true
    }

    fn request(&mut self, key: TileKey) {
        if !self.pending.insert(key) {
            return;
        }
        let terrain = Arc::clone(&self.terrain);
        let done = Arc::clone(&self.done);
        let generation = self.generation;
        let job = move || {
            // The collision shape (with its search tree) is built here too, off the
            // simulation thread.
            let patch = collision_patch(&terrain, key, PATCH_DETAIL);
            let Ok(shape) = SharedShape::trimesh(patch.vertices, patch.triangles) else {
                return;
            };
            if let Ok(mut done) = done.lock() {
                done.push((generation, key, shape));
            }
        };
        if self.inline {
            job();
        } else if let Ok(pool) = workers().lock() {
            let _ = pool.send(Box::new(job));
        }
    }

    /// Removes the ground body and all patches.
    pub(crate) fn remove(self, world: &mut PhysicsWorld) {
        world.remove_body(self.body);
    }

    /// Points the ground body along the planet's surface rotation (inertial frame), turning at
    /// `spin` (it is re-placed every tick, so it never drifts from the planet).
    pub(crate) fn place(&self, world: &mut PhysicsWorld, rotation: DQuat, spin: DVec3) {
        let body = &mut world.bodies[self.body];
        body.set_position(Pose::from_rotation(rotation), false);
        // Rapier's linear velocity is that of the centre of mass.
        let com = body.center_of_mass();
        body.set_linvel(spin.cross(com), false);
        body.set_angvel(spin, false);
    }

    /// Patch keys covering a disc of `radius` (m) around unit direction `dir` (surface frame).
    fn keys_around(&self, dir: DVec3, radius: f64) -> HashSet<TileKey> {
        let mut keys = HashSet::new();
        keys.insert(locate(dir, self.level));
        let east = DVec3::Y.cross(dir).normalize_or(DVec3::X);
        let north = dir.cross(east);
        let step = self.patch_size * 0.5;
        let n = (radius / step).ceil() as i32;
        for i in -n..=n {
            for j in -n..=n {
                let (x, y) = (f64::from(i) * step, f64::from(j) * step);
                if x * x + y * y > (radius + step) * (radius + step) {
                    continue;
                }
                let d = (dir * self.terrain.radius + east * x + north * y).normalize();
                keys.insert(locate(d, self.level));
            }
        }
        keys
    }

    /// Asks for the patches around unit direction `dir` (surface frame): marks them wanted at
    /// time `now` and starts building the missing ones. Returns whether every patch within
    /// `cover` (m) of `dir` is ready.
    pub(crate) fn want(&mut self, dir: DVec3, radius: f64, cover: f64, now: f64) -> bool {
        // Buildings: boxes are cheap, so they go in at once (no background build).
        if let Some(f) = self.terrain.foundations() {
            for (key, _) in f.structures_near(dir, radius) {
                match self.boxes.get_mut(&key) {
                    Some(b) => b.wanted_at = now,
                    None => {
                        self.boxes.insert(
                            key,
                            Patch {
                                collider: ColliderHandle::invalid(),
                                wanted_at: now,
                            },
                        );
                    }
                }
            }
        }
        for key in self.keys_around(dir, radius) {
            if let Some(p) = self.patches.get_mut(&key) {
                p.wanted_at = now;
            } else {
                self.request(key);
            }
        }
        self.keys_around(dir, cover)
            .iter()
            .all(|k| self.patches.contains_key(k))
    }

    /// Adds finished patches to the world and drops ones nobody wants any more.
    pub(crate) fn update(&mut self, world: &mut PhysicsWorld, now: f64) {
        // A few finished patches per tick keep the cost of inserting them small.
        let finished: Vec<_> = match self.done.lock() {
            Ok(mut done) => {
                let n = done.len().min(PATCHES_PER_TICK);
                done.drain(..n).collect()
            }
            Err(_) => Vec::new(),
        };
        for (generation, key, shape) in finished {
            if generation != self.generation || !self.pending.remove(&key) {
                continue; // built from an older terrain
            }
            // Replace the old patch (kept until now so nothing stood on a hole).
            if let Some(old) = self.patches.remove(&key) {
                world.remove_collider(old.collider);
            }
            let builder = ColliderBuilder::new(shape)
                // Massless, so the ground body's centre of mass stays at the planet's centre and
                // its velocity at any point is spin × r.
                .density(0.0)
                .friction(GROUND_FRICTION)
                .collision_groups(InteractionGroups::new(
                    GROUND_GROUP,
                    Group::ALL,
                    InteractionTestMode::And,
                ));
            let collider = world.insert_collider(builder, Some(self.body));
            self.patches.insert(
                key,
                Patch {
                    collider,
                    wanted_at: now,
                },
            );
        }
        // Buildings wanted for the first time get their colliders; unwanted ones go.
        if let Some(f) = self.terrain.foundations().cloned() {
            let sites = f.sites();
            for (&(site, k), b) in self.boxes.iter_mut() {
                if b.collider != ColliderHandle::invalid() {
                    continue;
                }
                let Some(s) = sites
                    .get(site as usize)
                    .and_then(|s| s.structures().get(k as usize))
                else {
                    continue;
                };
                let builder = ColliderBuilder::cuboid(s.half.x, s.half.y, s.half.z)
                    .position(Pose::from_parts(s.center, s.rotation))
                    .density(0.0)
                    .friction(GROUND_FRICTION)
                    .collision_groups(InteractionGroups::new(
                        GROUND_GROUP,
                        Group::ALL,
                        InteractionTestMode::And,
                    ));
                b.collider = world.insert_collider(builder, Some(self.body));
            }
        }
        let gone: Vec<(u32, u32)> = self
            .boxes
            .iter()
            .filter(|(_, b)| now - b.wanted_at > PATCH_LIFETIME)
            .map(|(k, _)| *k)
            .collect();
        for key in gone {
            if let Some(b) = self.boxes.remove(&key)
                && b.collider != ColliderHandle::invalid()
            {
                world.remove_collider(b.collider);
            }
        }
        let mut by_age: Vec<(f64, TileKey)> = self
            .patches
            .iter()
            .map(|(k, p)| (p.wanted_at, *k))
            .collect();
        by_age.sort_by(|a, b| a.0.total_cmp(&b.0));
        let excess = by_age.len().saturating_sub(MAX_PATCHES);
        let stale: Vec<TileKey> = by_age
            .iter()
            .enumerate()
            .filter(|(i, (wanted_at, _))| *i < excess || now - wanted_at > PATCH_LIFETIME)
            .map(|(_, (_, k))| *k)
            .collect();
        for key in stale {
            if let Some(p) = self.patches.remove(&key) {
                world.remove_collider(p.collider);
            }
        }
    }

    /// Number of patches being built.
    pub(crate) fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Number of buildings with colliders (stats and tests).
    pub(crate) fn buildings(&self) -> usize {
        self.boxes
            .values()
            .filter(|b| b.collider != ColliderHandle::invalid())
            .count()
    }

    /// Number of built patches (stats and tests).
    pub(crate) fn len(&self) -> usize {
        self.patches.len()
    }
}
