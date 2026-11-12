//! Ultra Space: Bevy front end.
//!
//! Placeholder scene until Batch 1. Game logic lives in the engine-independent
//! `sim_*` crates; this crate only renders, handles input and wires plugins together.

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Ultra Space".into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, scene.spawn())
        .run();
}

/// A placeholder "planet" and light, replaced in Batch 1.
fn scene() -> impl SceneList {
    bsn_list! {
        #Planet
        Mesh3d(asset_value(Sphere::new(1.0)))
        MeshMaterial3d::<StandardMaterial>(asset_value(Color::srgb_u8(90, 140, 200)))
        Transform::from_xyz(0.0, 0.0, 0.0)
        --
        DirectionalLight {
            shadow_maps_enabled: true,
        }
        Transform::from_xyz(4.0, 6.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y)
        --
        Camera3d
        Transform::from_xyz(0.0, 1.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y)
    }
}
