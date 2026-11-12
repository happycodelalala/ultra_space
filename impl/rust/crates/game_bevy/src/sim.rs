//! Runs the engine-independent `sim_core::Simulation` from the Bevy app.
//!
//! The simulation advances on its own fixed tick (30 Hz by default), decoupled from the render
//! frame rate. There is one region per celestial body.

use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use sim_core::{Simulation, StarSystem};

use crate::universe::InGame;

/// Minimum time between "over budget" warnings, so a slow stretch doesn't flood the log.
const BUDGET_WARNING_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Resource)]
pub struct Sim(pub Simulation);

/// Simulation time advance; camera and rendering read the updated universe time after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimSet;

pub struct SimPlugin;

/// Builds the simulation for a star system: one region per body, each running vehicle flight.
/// Runs on the loading worker (`universe`), not the main thread.
pub fn build_simulation(system: Arc<StarSystem>) -> Simulation {
    let mut sim = Simulation::for_star_system(system, |_, schedule| {
        sim_vehicle::flight::add_flight_systems(schedule);
    });
    // Components that travel with objects between regions.
    sim.objects.register_transferable::<sim_vehicle::Vehicle>();
    sim.objects
        .register_transferable::<sim_vehicle::VehicleBody>();
    sim.objects
        .register_transferable::<sim_vehicle::FlightControl>();
    sim.objects
        .register_transferable::<sim_vehicle::FlightTelemetry>();
    sim.objects.register_transferable::<sim_vehicle::Cruise>();
    sim.objects
        .register_transferable::<sim_vehicle::ArcadeFlight>();
    sim.objects.register_transferable::<sim_vehicle::Walker>();
    sim.objects
        .register_transferable::<sim_vehicle::WalkerControl>();
    // Vehicles land on the real terrain (built once per region, on the loading worker).
    sim_vehicle::flight::install_terrain(&mut sim);
    sim
}

/// Universe time (s) shown this frame: between the last two ticks (see
/// `Simulation::render_time`). Everything drawn uses it instead of the simulation's own time,
/// so motion is smooth at any frame rate.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct RenderTime(pub f64);

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderTime>()
            .configure_sets(Update, SimSet.in_set(InGame))
            .add_systems(
                Update,
                (time_warp_keys, advance_simulation, update_render_time)
                    .chain()
                    .in_set(SimSet),
            );
    }
}

fn update_render_time(sim: Res<Sim>, mut render: ResMut<RenderTime>) {
    render.0 = sim.0.render_time();
}

fn advance_simulation(
    time: Res<Time>,
    real: Res<Time<Real>>,
    mut sim: ResMut<Sim>,
    mut last_warning: Local<Option<Duration>>,
) {
    let ticks = sim.0.advance(time.delta());
    if ticks == 0 {
        return;
    }
    let budget = sim.0.clock().tick_duration();
    if let Some(last) = sim.0.tick_stats().last()
        && last > budget
    {
        let now = real.elapsed();
        if last_warning.is_none_or(|t| now - t >= BUDGET_WARNING_INTERVAL) {
            *last_warning = Some(now);
            warn!(
                "simulation tick over budget: {:.2} ms (budget {:.2} ms)",
                last.as_secs_f64() * 1e3,
                budget.as_secs_f64() * 1e3
            );
        }
    }
}

/// `.` speeds up time warp, `,` slows it down (debug; KSP-style keys).
fn time_warp_keys(keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<Sim>) {
    let before = sim.0.time_warp();
    if keys.just_pressed(KeyCode::Period) {
        sim.0.step_time_warp(true);
    }
    if keys.just_pressed(KeyCode::Comma) {
        sim.0.step_time_warp(false);
    }
    if sim.0.time_warp() != before {
        info!("time warp ×{}", sim.0.time_warp());
    }
}
