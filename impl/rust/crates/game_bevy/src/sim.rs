//! Runs the engine-independent `sim_core::Simulation` from the Bevy app.
//!
//! The simulation advances on its own fixed tick (30 Hz by default), decoupled from the render
//! frame rate. Regions are placeholders until Batch 1 adds real content.

use std::time::Duration;

use bevy::prelude::*;
use sim_core::{Region, RegionSet, Simulation};

/// Placeholder region count until real star systems exist (Batch 1).
const PLACEHOLDER_REGIONS: usize = 8;

/// Minimum time between "over budget" warnings, so a slow stretch doesn't flood the log.
const BUDGET_WARNING_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Resource)]
pub struct Sim(pub Simulation);

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        let mut regions = RegionSet::new();
        for i in 0..PLACEHOLDER_REGIONS {
            regions.push(Region::empty(format!("region-{i}")));
        }
        let sim = Simulation::new(regions);
        info!(
            "simulation: {} regions at {:.0} Hz",
            sim.regions.len(),
            sim.clock().tick_hz()
        );
        app.insert_resource(Sim(sim))
            .add_systems(Update, advance_simulation);
    }
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
