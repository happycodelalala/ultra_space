//! Start-up warm-up (Batch 2, task 2.12; overview §14.4 "warm up GPU pipelines").
//!
//! The first frames compile every shader pipeline the scene uses and stream the terrain around
//! the camera, which makes them slow. That work happens behind a full-screen "Loading" overlay:
//! the world is shown once no pipelines are waiting to compile and the terrain near the camera
//! is on screen, both for `SETTLE_FRAMES` frames in a row (or after `TIMEOUT`). [`WarmupDone`]
//! then exists; the start-up hitch summary, test scripts and screenshot captures count from it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy::prelude::*;
use bevy::render::render_resource::{CachedPipelineState, PipelineCache};
use bevy::render::{Render, RenderApp, RenderSystems};

use crate::terrain::{Terrains, terrain_ready};
use crate::universe::GameState;

/// Frames in a row the scene must be ready before it's shown.
const SETTLE_FRAMES: u32 = 20;
/// Show the world anyway after this long (s) of running.
const TIMEOUT: f64 = 30.0;

pub struct WarmupPlugin;

impl Plugin for WarmupPlugin {
    fn build(&self, app: &mut App) {
        let backlog = PipelineBacklog(Arc::new(AtomicUsize::new(usize::MAX)));
        app.insert_resource(backlog.clone())
            .init_resource::<Warmup>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(
                Update,
                (settle, remove_primers).run_if(in_state(GameState::Running)),
            );
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(backlog)
                .add_systems(Render, count_backlog.in_set(RenderSystems::Cleanup));
        }
    }
}

/// Pipelines not yet compiled, shared between the render world (writes) and the game (reads).
#[derive(Resource, Clone)]
struct PipelineBacklog(Arc<AtomicUsize>);

/// Exists once the world is shown (see the module docs).
#[derive(Resource)]
pub struct WarmupDone;

/// A primer: something drawn during the warm-up only so the pipelines of what appears later
/// (settlement materials, night glow, lanterns, people) are compiled behind the overlay
/// rather than in play. Spawned as a tiny child of the main camera; removed once the world is
/// shown.
#[derive(Component)]
pub struct Primer;

/// Puts `bundle` as a primer just in front of the main camera (when there is one).
pub fn spawn_primer(commands: &mut Commands, camera: Option<Entity>, bundle: impl Bundle) {
    let Some(camera) = camera else { return };
    commands.spawn((
        Primer,
        bundle,
        Transform::from_xyz(0.0, 0.0, -2.0).with_scale(Vec3::splat(0.002)),
        ChildOf(camera),
    ));
}

fn remove_primers(
    mut commands: Commands,
    done: Option<Res<WarmupDone>>,
    primers: Query<Entity, With<Primer>>,
) {
    if done.is_some() {
        for e in &primers {
            commands.entity(e).despawn();
        }
    }
}

#[derive(Resource, Default)]
struct Warmup {
    started: Option<f64>,
    ready_frames: u32,
}

#[derive(Component)]
struct Overlay;

fn count_backlog(cache: Res<PipelineCache>, backlog: Res<PipelineBacklog>) {
    let waiting = cache.waiting_pipelines().count()
        + cache
            .pipelines()
            .filter(|p| {
                matches!(
                    p.state,
                    CachedPipelineState::Queued | CachedPipelineState::Creating(_)
                )
            })
            .count();
    backlog.0.store(waiting, Ordering::Relaxed);
}

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Overlay,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(1_000),
        children![(
            Text::new("Loading..."),
            TextFont {
                font_size: FontSize::Px(22.0),
                ..default()
            },
        )],
    ));
}

fn settle(
    mut commands: Commands,
    mut warmup: ResMut<Warmup>,
    real: Res<Time<Real>>,
    backlog: Res<PipelineBacklog>,
    terrains: Option<Res<Terrains>>,
    done: Option<Res<WarmupDone>>,
    overlay: Query<Entity, With<Overlay>>,
) {
    if done.is_some() {
        return;
    }
    let now = real.elapsed_secs_f64();
    let started = *warmup.started.get_or_insert(now);
    let pending = backlog.0.load(Ordering::Relaxed);
    if pending == 0 && terrain_ready(terrains.as_deref()) {
        warmup.ready_frames += 1;
    } else {
        warmup.ready_frames = 0;
    }
    let timed_out = now - started > TIMEOUT;
    if warmup.ready_frames >= SETTLE_FRAMES || timed_out {
        if timed_out {
            warn!(
                "warm-up: showing the world after {TIMEOUT:.0} s with {pending} pipelines still compiling"
            );
        } else {
            info!("warm-up: world shown after {:.2} s", now - started);
        }
        for e in &overlay {
            commands.entity(e).despawn();
        }
        commands.insert_resource(WarmupDone);
    }
}
