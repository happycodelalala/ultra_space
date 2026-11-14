//! Meeting the world (task 4.9; living-planet.md §7):
//!
//! - **Entering a settlement** (the camera within its extent) shows its name, kind, faction
//!   and culture in a banner for a few seconds.
//! - **The inspector** (F4) shows the settlement the camera is in or nearest (within 3 km):
//!   its live state from the aggregate simulation (people by role, food, goods, mood) and its
//!   lore.
//! - **Looking at a person** (the person nearest the middle of the view, within 30 m) shows
//!   their name, their role in their culture's word, and their culture.

use std::fmt::Write;
use std::time::Duration;

use bevy::prelude::*;
use glam::DVec3;
use sim_society::RegionSociety;

use crate::camera::SpaceCamera;
use crate::people::PersonLabel;
use crate::settlements::{SettlementDraw, SettlementSet, camera_on};
use crate::sim::{RenderTime, Sim};
use crate::space_render::MainCamera;
use crate::universe::{InGame, Universe};

/// How long the banner stays (s).
const BANNER_TIME: f32 = 6.0;
/// Settlements this far beyond their extent (m) still show in the inspector.
const INSPECT_RANGE: f64 = 3_000.0;
/// People this far (m) or this far off the line of sight (m) aren't picked.
const LOOK_RANGE: f32 = 30.0;
const LOOK_WIDTH: f32 = 0.6;

pub struct MeetingPlugin;

impl Plugin for MeetingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Meeting>()
            .init_resource::<SettlementInfo>()
            .add_systems(Startup, spawn_labels)
            .add_systems(
                Update,
                (entering, inspect, look_at)
                    .after(SettlementSet)
                    .in_set(InGame),
            );
    }
}

#[derive(Resource, Default)]
struct Meeting {
    /// The settlement (drawn site index) the camera is in.
    inside: Option<usize>,
    banner_left: f32,
    since_inspect: Duration,
}

/// The inspector's settlement section (empty when none is near).
#[derive(Resource, Default)]
pub struct SettlementInfo(pub String);

#[derive(Component)]
struct Banner;

#[derive(Component)]
struct LookLabel;

fn spawn_labels(mut commands: Commands) {
    commands.spawn((
        Banner,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(26.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.95, 0.8)),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            top: px(70),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Visibility::Hidden,
    ));
    commands.spawn((
        LookLabel,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.95, 0.9)),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            top: percent(55),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Visibility::Hidden,
    ));
}

/// The camera's distance (m) beyond each settlement's extent, on the camera's body.
fn distances(
    draw: &SettlementDraw,
    universe: &Universe,
    camera: &SpaceCamera,
    t: f64,
) -> Vec<(usize, f64)> {
    let mut cams: Vec<(usize, DVec3)> = Vec::new();
    draw.sites
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let cam = match cams.iter().find(|c| c.0 == s.body) {
                Some(c) => c.1,
                None => {
                    let c = camera_on(universe, camera, s.body, t);
                    cams.push((s.body, c));
                    c
                }
            };
            (i, cam.distance(s.origin) - s.extent)
        })
        .collect()
}

fn entering(
    time: Res<Time>,
    draw: Option<Res<SettlementDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut meeting: ResMut<Meeting>,
    mut banner: Query<(&mut Text, &mut Visibility), With<Banner>>,
) {
    let Some(draw) = draw else { return };
    let near = distances(&draw, &universe, &camera, render.0);
    let inside = near
        .iter()
        .filter(|(_, d)| *d < 0.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|n| n.0);
    // Leaving needs a margin, so the edge doesn't flicker the banner.
    let still = meeting
        .inside
        .is_some_and(|i| near.get(i).is_some_and(|n| n.1 < 200.0));
    if inside.is_some() && inside != meeting.inside && !still {
        let s = &draw.sites[inside.unwrap_or(0)];
        let st = &s.settlement;
        let seat = if st.seat { ", its seat" } else { "" };
        if let Ok((mut text, _)) = banner.single_mut() {
            text.0 = format!(
                "{}\n{} {} of {}{seat}\n{} people",
                st.name.to_uppercase(),
                article(&s.culture.1),
                st.kind.name(),
                // "of the Gostkod March" mid-sentence.
                s.faction
                    .strip_prefix("The ")
                    .map_or(s.faction.clone(), |f| format!("the {f}")),
                st.population
            );
        }
        meeting.inside = inside;
        meeting.banner_left = BANNER_TIME;
        info!(
            "entered {} ({} {}, {})",
            st.name,
            s.culture.1,
            st.kind.name(),
            s.faction
        );
    } else if inside.is_none() && !still {
        meeting.inside = None;
    }
    meeting.banner_left = (meeting.banner_left - time.delta_secs()).max(0.0);
    if let Ok((_, mut v)) = banner.single_mut() {
        let want = if meeting.banner_left > 0.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
    }
}

/// "A Hollish" or "An Ostran".
fn article(adjective: &str) -> String {
    let vowel = adjective
        .chars()
        .next()
        .is_some_and(|c| "AEIOUaeiou".contains(c));
    format!("{} {adjective}", if vowel { "An" } else { "A" })
}

/// Keeps the inspector's settlement section up to date (four times a second).
#[allow(clippy::too_many_arguments)]
fn inspect(
    real: Res<Time<Real>>,
    draw: Option<Res<SettlementDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    sim: Res<Sim>,
    mut meeting: ResMut<Meeting>,
    mut info: ResMut<SettlementInfo>,
) {
    meeting.since_inspect += real.delta();
    if meeting.since_inspect < Duration::from_millis(250) {
        return;
    }
    meeting.since_inspect = Duration::ZERO;
    let Some(draw) = draw else { return };
    let nearest = distances(&draw, &universe, &camera, render.0)
        .into_iter()
        .filter(|n| n.1 < INSPECT_RANGE)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((i, distance)) = nearest else {
        if !info.0.is_empty() {
            info.0.clear();
        }
        return;
    };
    let site = &draw.sites[i];
    let s = &site.settlement;
    let mut out = String::new();
    let where_ = if distance < 0.0 {
        "here".to_string()
    } else {
        format!("{:.1} km away", distance / 1000.0)
    };
    let _ = writeln!(out, "Settlement: {} ({where_})", s.name);
    let _ = writeln!(
        out,
        "  {} {}, {}{}, {} people, founded {} years ago",
        site.culture.1,
        s.kind.name(),
        site.faction,
        if s.seat { " (seat)" } else { "" },
        s.population,
        s.founded_years_ago
    );
    // Live state from the aggregate simulation.
    if let Some(rs) = sim
        .0
        .regions
        .get(site.body)
        .and_then(|r| r.world.get_resource::<RegionSociety>())
        && let Some(k) = rs.settlements.iter().position(|x| x.id == s.id)
    {
        let st = &rs.states[k];
        let people = st.population();
        let _ = writeln!(
            out,
            "  now {:.0} people, food {:.0} days, goods {:.0} days, mood {:.2}",
            people,
            st.food / people.max(1.0),
            st.goods / (people.max(1.0) * 0.3),
            st.mood
        );
        let roles: Vec<String> = rs.settlements[k]
            .roles
            .iter()
            .zip(&st.people)
            .filter(|(_, n)| **n >= 0.5)
            .map(|((r, _), n)| format!("{} {:.0}", site.role_names.get(r).unwrap_or(r), n))
            .collect();
        let _ = writeln!(out, "  {}", roles.join(", "));
    }
    let _ = writeln!(out, "  {}", s.lore.description);
    let _ = writeln!(out, "  Landmark: {}", s.lore.landmark);
    if info.0 != out {
        info.0 = out;
    }
}

/// Whether a building stands between surface-frame points `a` and `b`.
fn blocked(foundations: &sim_planet::Foundations, a: DVec3, b: DVec3) -> bool {
    let mid = (0.5 * (a + b)).normalize();
    for (_, s) in foundations.structures_near(mid, 0.5 * a.distance(b)) {
        // Slab test in the box's own axes.
        let inv = s.rotation.inverse();
        let (o, d) = (inv * (a - s.center), inv * (b - a));
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut hit = true;
        for k in 0..3 {
            let (o, d, h) = (o[k], d[k], s.half[k]);
            if d.abs() < 1e-9 {
                if o.abs() > h {
                    hit = false;
                    break;
                }
            } else {
                let (u, v) = ((-h - o) / d, (h - o) / d);
                t0 = t0.max(u.min(v));
                t1 = t1.min(u.max(v));
                if t0 > t1 {
                    hit = false;
                    break;
                }
            }
        }
        if hit {
            return true;
        }
    }
    false
}

/// Names the person nearest the middle of the view (not behind a building).
#[allow(clippy::too_many_arguments)]
fn look_at(
    draw: Option<Res<SettlementDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    cameras: Query<&GlobalTransform, With<MainCamera>>,
    people: Query<(&GlobalTransform, &InheritedVisibility, &PersonLabel)>,
    mut label: Query<(&mut Text, &mut Visibility), With<LookLabel>>,
) {
    let Ok(cam) = cameras.single() else { return };
    // Render space (camera-relative, anchor axes) to the camera body's surface frame.
    let system = &universe.0;
    let body = draw
        .as_ref()
        .and_then(|d| d.sites.first())
        .map_or(camera.anchor, |s| s.body);
    let surface = system.frames.state_in(
        system.bodies[camera.anchor].inertial_frame,
        system.bodies[body].surface_frame,
        render.0,
    );
    let to_surface =
        |v: Vec3| surface.rotation.inverse() * (v.as_dvec3() + camera.offset - surface.position);
    let foundations = draw
        .as_ref()
        .and_then(|d| d.sites.first())
        .and_then(|s| s.terrain.foundations().cloned());
    let (origin, forward) = (cam.translation(), cam.forward().as_vec3());
    let mut best: Option<(f32, &str)> = None;
    for (t, visible, l) in &people {
        if !visible.get() {
            continue;
        }
        // Their chest, about a metre up (people stand upright in their own frame).
        let chest = t.translation() + t.up().as_vec3() * 1.1;
        let d = chest - origin;
        let along = d.dot(forward);
        if !(0.5..LOOK_RANGE).contains(&along) {
            continue;
        }
        let off = (d - forward * along).length();
        if off < LOOK_WIDTH
            && best.is_none_or(|b| along < b.0)
            && !foundations
                .as_ref()
                .is_some_and(|f| blocked(f, to_surface(origin), to_surface(chest)))
        {
            best = Some((along, &l.0));
        }
    }
    if let Ok((mut text, mut v)) = label.single_mut() {
        match best {
            Some((_, name)) => {
                if text.0 != name {
                    text.0 = name.to_string();
                }
                if *v != Visibility::Visible {
                    *v = Visibility::Visible;
                }
            }
            None => {
                if *v != Visibility::Hidden {
                    *v = Visibility::Hidden;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_building_blocks_the_line_of_sight() {
        use sim_planet::{Foundations, PlacedBox, Site, tangent_frame};
        let r = 600_000.0;
        let c = DVec3::new(0.3, 0.8, 0.2).normalize();
        let site = Site::with_boxes(
            c,
            Vec::new(),
            vec![PlacedBox {
                center: DVec3::new(0.0, 103.0, -20.0),
                yaw: 0.4,
                half: DVec3::new(5.0, 3.0, 4.0),
            }],
            4.0,
            r,
        );
        let f = Foundations::new(vec![site]);
        let (e, s) = tangent_frame(c);
        // A point x east, z south, h above the reference radius.
        let at = |x: f64, z: f64, h: f64| (c * r + e * x + s * z).normalize() * (r + h);
        // Through the building (north of the eye, 1.6 m up at its floor height of 100 m)…
        assert!(blocked(&f, at(0.0, 0.0, 101.6), at(0.0, -40.0, 101.6)));
        // …past its side, and over its roof.
        assert!(!blocked(&f, at(20.0, 0.0, 101.6), at(20.0, -40.0, 101.6)));
        assert!(!blocked(&f, at(0.0, 0.0, 110.0), at(0.0, -40.0, 110.0)));
    }

    #[test]
    fn articles_follow_the_adjective() {
        assert_eq!(article("Hollish"), "A Hollish");
        assert_eq!(article("Ostran"), "An Ostran");
    }
}
