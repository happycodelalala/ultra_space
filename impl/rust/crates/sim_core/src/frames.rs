//! Frame tree (Batch 1, task 1.2; technical overview §1).
//!
//! Every position in the universe is relative to a **frame**: star, planet, moon and so on.
//! Frames form a tree; each frame moves relative to its parent (fixed offset, orbit or spin).
//! The state of any frame relative to any other is computed in `f64` through their lowest
//! common ancestor, so nearby things stay precise however far they are from the root.
//!
//! A celestial body normally gets **two** frames:
//! - an **inertial** frame that follows its orbit but doesn't rotate (ships in orbit and its
//!   sphere of influence live here), and
//! - a **surface** frame, a [`Spin`] child of the inertial frame that rotates with the ground
//!   (terrain, cities and anything standing on it live here).

use std::f64::consts::TAU;
use std::fmt;

use glam::{DQuat, DVec3};

use crate::orbit::KeplerOrbit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(u32);

impl FrameId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for FrameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame#{}", self.0)
    }
}

/// Constant rotation about an axis tilted relative to the parent frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spin {
    /// Orientation of the unspun body in the parent frame; its local +Y is the spin axis
    /// (north pole). Axial tilt is encoded here.
    pub pole: DQuat,
    /// Sidereal rotation period (s). Negative means retrograde spin.
    pub period: f64,
    /// Rotation angle about the pole at universe time 0 (rad).
    pub phase_at_epoch: f64,
}

impl Spin {
    /// Spin with the pole tilted by `tilt` radians about the parent's X axis.
    pub fn tilted(tilt: f64, period: f64, phase_at_epoch: f64) -> Self {
        Self {
            pole: DQuat::from_rotation_x(tilt),
            period,
            phase_at_epoch,
        }
    }

    /// Spin of a body tidally locked to its (circular) orbit: the spin axis is the orbit
    /// normal and the period is the orbital period, so the body's local -X always faces its
    /// parent. On eccentric orbits the face wobbles slightly (libration), as in reality.
    pub fn tidally_locked(orbit: &KeplerOrbit) -> Self {
        Self {
            pole: orbit.orientation(),
            period: orbit.period(),
            phase_at_epoch: orbit.mean_anomaly_at_epoch,
        }
    }

    pub fn angle(&self, t: f64) -> f64 {
        self.phase_at_epoch + TAU * t / self.period
    }

    pub fn rotation(&self, t: f64) -> DQuat {
        self.pole * DQuat::from_rotation_y(self.angle(t))
    }

    /// Angular velocity in the parent frame (rad/s).
    pub fn angular_velocity(&self) -> DVec3 {
        self.pole * DVec3::Y * (TAU / self.period)
    }
}

/// How a frame moves relative to its parent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FrameMotion {
    /// Fixed offset and orientation.
    Fixed { position: DVec3, rotation: DQuat },
    /// Follows a Kepler orbit; doesn't rotate relative to the parent.
    Orbit(KeplerOrbit),
    /// Same origin as the parent, rotating.
    Spin(Spin),
}

/// Position, orientation, velocity and angular velocity of a frame relative to a reference
/// frame, all expressed in the reference frame's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameState {
    pub position: DVec3,
    /// Rotates vectors from the frame's coordinates into the reference frame's.
    pub rotation: DQuat,
    pub velocity: DVec3,
    pub angular_velocity: DVec3,
}

impl FrameState {
    pub const IDENTITY: Self = Self {
        position: DVec3::ZERO,
        rotation: DQuat::IDENTITY,
        velocity: DVec3::ZERO,
        angular_velocity: DVec3::ZERO,
    };

    /// Given `self` = state of B in A and `child` = state of C in B, returns state of C in A.
    pub fn compose(&self, child: &FrameState) -> FrameState {
        let offset = self.rotation * child.position;
        FrameState {
            position: self.position + offset,
            rotation: (self.rotation * child.rotation).normalize(),
            velocity: self.velocity
                + self.angular_velocity.cross(offset)
                + self.rotation * child.velocity,
            angular_velocity: self.angular_velocity + self.rotation * child.angular_velocity,
        }
    }

    /// Given `self` = state of B in A, returns state of A in B.
    pub fn inverse(&self) -> FrameState {
        let inv = self.rotation.inverse();
        FrameState {
            position: -(inv * self.position),
            rotation: inv,
            velocity: inv * (self.angular_velocity.cross(self.position) - self.velocity),
            angular_velocity: -(inv * self.angular_velocity),
        }
    }

    /// A point given in the frame's coordinates, expressed in the reference frame.
    pub fn transform_point(&self, point: DVec3) -> DVec3 {
        self.position + self.rotation * point
    }

    /// A direction given in the frame's coordinates, expressed in the reference frame.
    pub fn transform_vector(&self, vector: DVec3) -> DVec3 {
        self.rotation * vector
    }

    /// Velocity in the reference frame of a point at `point` moving with `velocity`, both
    /// given in the frame's coordinates.
    pub fn transform_velocity(&self, point: DVec3, velocity: DVec3) -> DVec3 {
        self.velocity
            + self.angular_velocity.cross(self.rotation * point)
            + self.rotation * velocity
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub name: String,
    pub parent: Option<FrameId>,
    pub motion: FrameMotion,
    depth: u32,
}

impl Frame {
    /// Distance from the root (the root has depth 0).
    pub fn depth(&self) -> u32 {
        self.depth
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    UnknownFrame(FrameId),
    DuplicateName(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFrame(id) => write!(f, "unknown frame {id}"),
            Self::DuplicateName(name) => write!(f, "a frame named {name:?} already exists"),
        }
    }
}

impl std::error::Error for FrameError {}

/// All frames of a universe (or star system). Frames are only added, never removed, so
/// `FrameId`s stay valid.
#[derive(Clone, Debug)]
pub struct FrameTree {
    frames: Vec<Frame>,
}

impl FrameTree {
    /// A tree with a single root frame (for example the galaxy or a star system barycenter).
    pub fn new(root_name: impl Into<String>) -> Self {
        Self {
            frames: vec![Frame {
                name: root_name.into(),
                parent: None,
                motion: FrameMotion::Fixed {
                    position: DVec3::ZERO,
                    rotation: DQuat::IDENTITY,
                },
                depth: 0,
            }],
        }
    }

    pub fn root(&self) -> FrameId {
        FrameId(0)
    }

    pub fn add(
        &mut self,
        name: impl Into<String>,
        parent: FrameId,
        motion: FrameMotion,
    ) -> Result<FrameId, FrameError> {
        let name = name.into();
        let parent_depth = self
            .get(parent)
            .ok_or(FrameError::UnknownFrame(parent))?
            .depth;
        if self.find(&name).is_some() {
            return Err(FrameError::DuplicateName(name));
        }
        let id = FrameId(u32::try_from(self.frames.len()).expect("too many frames"));
        self.frames.push(Frame {
            name,
            parent: Some(parent),
            motion,
            depth: parent_depth + 1,
        });
        Ok(id)
    }

    pub fn get(&self, id: FrameId) -> Option<&Frame> {
        self.frames.get(id.index())
    }

    fn frame(&self, id: FrameId) -> &Frame {
        self.get(id).unwrap_or_else(|| panic!("unknown frame {id}"))
    }

    pub fn find(&self, name: &str) -> Option<FrameId> {
        self.frames
            .iter()
            .position(|f| f.name == name)
            .map(|i| FrameId(i as u32))
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (FrameId, &Frame)> {
        self.frames
            .iter()
            .enumerate()
            .map(|(i, f)| (FrameId(i as u32), f))
    }

    /// Children of `id`, in insertion order.
    pub fn children(&self, id: FrameId) -> impl Iterator<Item = FrameId> + '_ {
        self.iter()
            .filter(move |(_, f)| f.parent == Some(id))
            .map(|(child, _)| child)
    }

    /// State of `id` relative to its parent at universe time `t` (identity for the root).
    pub fn local_state(&self, id: FrameId, t: f64) -> FrameState {
        let frame = self.frame(id);
        if frame.parent.is_none() {
            return FrameState::IDENTITY;
        }
        match frame.motion {
            FrameMotion::Fixed { position, rotation } => FrameState {
                position,
                rotation,
                ..FrameState::IDENTITY
            },
            FrameMotion::Orbit(orbit) => {
                let (position, velocity) = orbit.state_at(t);
                FrameState {
                    position,
                    velocity,
                    ..FrameState::IDENTITY
                }
            }
            FrameMotion::Spin(spin) => FrameState {
                rotation: spin.rotation(t),
                angular_velocity: spin.angular_velocity(),
                ..FrameState::IDENTITY
            },
        }
    }

    /// Lowest common ancestor of two frames.
    pub fn common_ancestor(&self, a: FrameId, b: FrameId) -> FrameId {
        let (mut a, mut b) = (a, b);
        while self.frame(a).depth > self.frame(b).depth {
            a = self.frame(a).parent.expect("non-root has a parent");
        }
        while self.frame(b).depth > self.frame(a).depth {
            b = self.frame(b).parent.expect("non-root has a parent");
        }
        while a != b {
            a = self.frame(a).parent.expect("non-root has a parent");
            b = self.frame(b).parent.expect("non-root has a parent");
        }
        a
    }

    /// Whether `ancestor` is `id` itself or one of its ancestors.
    pub fn is_ancestor_or_self(&self, ancestor: FrameId, id: FrameId) -> bool {
        let mut f = Some(id);
        while let Some(current) = f {
            if current == ancestor {
                return true;
            }
            f = self.frame(current).parent;
        }
        false
    }

    /// State of `id` relative to its ancestor `ancestor`.
    fn state_below(&self, ancestor: FrameId, id: FrameId, t: f64) -> FrameState {
        let mut state = FrameState::IDENTITY;
        let mut current = id;
        while current != ancestor {
            state = self.local_state(current, t).compose(&state);
            current = self.frame(current).parent.expect("ancestor is above id");
        }
        state
    }

    /// State of `frame` expressed in `reference` at universe time `t`.
    pub fn state_in(&self, reference: FrameId, frame: FrameId, t: f64) -> FrameState {
        let lca = self.common_ancestor(reference, frame);
        let reference_in_lca = self.state_below(lca, reference, t);
        let frame_in_lca = self.state_below(lca, frame, t);
        reference_in_lca.inverse().compose(&frame_in_lca)
    }

    /// A point given in `frame` coordinates, expressed in `reference` coordinates.
    pub fn transform_point(
        &self,
        reference: FrameId,
        frame: FrameId,
        t: f64,
        point: DVec3,
    ) -> DVec3 {
        self.state_in(reference, frame, t).transform_point(point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{KM, STANDARD_GRAVITY, deg, mu_from_surface_gravity};

    fn vec_close(a: DVec3, b: DVec3, tol: f64) -> bool {
        (a - b).length() <= tol
    }

    /// Star → planet (inertial) → planet surface (spin) → city; planet → moon → moon surface.
    struct System {
        tree: FrameTree,
        planet: FrameId,
        surface: FrameId,
        city: FrameId,
        moon: FrameId,
        moon_surface: FrameId,
    }

    fn system() -> System {
        let star_mu = 1.0e20;
        let planet_r = 600.0 * KM;
        let planet_mu = mu_from_surface_gravity(planet_r, STANDARD_GRAVITY);
        let mut tree = FrameTree::new("star");
        let star = tree.root();
        let planet_orbit = KeplerOrbit {
            mu: star_mu,
            semi_major_axis: 1.3e10,
            eccentricity: 0.02,
            inclination: deg(1.5),
            ascending_node: deg(20.0),
            arg_periapsis: deg(60.0),
            mean_anomaly_at_epoch: deg(5.0),
        };
        let planet = tree
            .add("planet", star, FrameMotion::Orbit(planet_orbit))
            .unwrap();
        let surface = tree
            .add(
                "planet.surface",
                planet,
                FrameMotion::Spin(Spin::tilted(deg(23.0), 5_400.0, 0.3)),
            )
            .unwrap();
        let city = tree
            .add(
                "city",
                surface,
                FrameMotion::Fixed {
                    position: DVec3::new(planet_r, 0.0, 0.0),
                    rotation: DQuat::from_rotation_z(deg(-90.0)),
                },
            )
            .unwrap();
        let moon_orbit = KeplerOrbit::circular(planet_mu, 12_000.0 * KM, deg(5.0), 0.0, 1.0);
        let moon = tree
            .add("moon", planet, FrameMotion::Orbit(moon_orbit))
            .unwrap();
        // Tidally locked, with the orbit inclined 5°: the spin axis follows the orbit normal.
        let moon_surface = tree
            .add(
                "moon.surface",
                moon,
                FrameMotion::Spin(Spin::tidally_locked(&moon_orbit)),
            )
            .unwrap();
        System {
            tree,
            planet,
            surface,
            city,
            moon,
            moon_surface,
        }
    }

    fn states_close(a: &FrameState, b: &FrameState, pos_tol: f64, vel_tol: f64) -> bool {
        vec_close(a.position, b.position, pos_tol)
            && a.rotation.angle_between(b.rotation) < 1e-9
            && vec_close(a.velocity, b.velocity, vel_tol)
            && vec_close(a.angular_velocity, b.angular_velocity, 1e-12)
    }

    #[test]
    fn building_and_lookup() {
        let s = system();
        assert_eq!(s.tree.find("city"), Some(s.city));
        assert_eq!(s.tree.get(s.city).unwrap().depth(), 3);
        assert_eq!(
            s.tree.children(s.planet).collect::<Vec<_>>(),
            vec![s.surface, s.moon]
        );
        let mut tree = s.tree.clone();
        assert_eq!(
            tree.add(
                "city",
                s.planet,
                FrameMotion::Spin(Spin::tilted(0.0, 1.0, 0.0))
            ),
            Err(FrameError::DuplicateName("city".into()))
        );
        assert_eq!(
            tree.add(
                "x",
                FrameId(999),
                FrameMotion::Spin(Spin::tilted(0.0, 1.0, 0.0))
            ),
            Err(FrameError::UnknownFrame(FrameId(999)))
        );
    }

    #[test]
    fn common_ancestors() {
        let s = system();
        assert_eq!(s.tree.common_ancestor(s.city, s.moon_surface), s.planet);
        assert_eq!(s.tree.common_ancestor(s.city, s.surface), s.surface);
        assert_eq!(s.tree.common_ancestor(s.city, s.tree.root()), s.tree.root());
        assert!(s.tree.is_ancestor_or_self(s.planet, s.city));
        assert!(!s.tree.is_ancestor_or_self(s.moon, s.city));
    }

    #[test]
    fn self_relative_state_is_identity() {
        let s = system();
        let st = s.tree.state_in(s.city, s.city, 1234.0);
        assert!(states_close(&st, &FrameState::IDENTITY, 1e-6, 1e-9));
    }

    #[test]
    fn inverse_and_reverse_are_consistent() {
        let s = system();
        let t = 4321.0;
        for &(a, b) in &[
            (s.city, s.moon_surface),
            (s.tree.root(), s.city),
            (s.moon, s.surface),
        ] {
            let ab = s.tree.state_in(a, b, t);
            let ba = s.tree.state_in(b, a, t);
            assert!(states_close(&ab.inverse(), &ba, 1e-3, 1e-6), "{a} {b}");
            assert!(
                states_close(&ab.compose(&ba), &FrameState::IDENTITY, 1e-3, 1e-6),
                "{a} {b}"
            );
        }
    }

    #[test]
    fn city_moves_with_the_ground() {
        let s = system();
        // Relative to the planet's inertial frame the city sits on the surface and moves at
        // equatorial speed 2πR / day.
        let st = s.tree.state_in(s.planet, s.city, 100.0);
        assert!((st.position.length() - 600.0 * KM).abs() < 1e-6);
        let expected = TAU * 600.0 * KM / 5_400.0;
        assert!(
            (st.velocity.length() - expected).abs() < 1e-6,
            "{}",
            st.velocity.length()
        );
    }

    #[test]
    fn velocities_match_finite_differences() {
        let s = system();
        let t = 2_000.0;
        let h = 1e-3;
        let pairs = [
            (s.tree.root(), s.city),
            (s.moon_surface, s.city),
            (s.city, s.moon),
            (s.surface, s.moon_surface),
        ];
        for &(reference, frame) in &pairs {
            let p = |t: f64| s.tree.state_in(reference, frame, t).position;
            let fd = (p(t + h) - p(t - h)) / (2.0 * h);
            let v = s.tree.state_in(reference, frame, t).velocity;
            let tol = 1e-6 * v.length().max(1.0) + 1e-3;
            assert!(vec_close(fd, v, tol), "{reference} {frame}: {fd} vs {v}");

            // Angular velocity: rotation change over a small step.
            let q0 = s.tree.state_in(reference, frame, t - h).rotation;
            let q1 = s.tree.state_in(reference, frame, t + h).rotation;
            let (axis, angle) = (q1 * q0.inverse()).to_axis_angle();
            let w_fd = axis * angle / (2.0 * h);
            let w = s.tree.state_in(reference, frame, t).angular_velocity;
            assert!(
                vec_close(w_fd, w, 1e-9),
                "{reference} {frame}: {w_fd} vs {w}"
            );
        }
    }

    #[test]
    fn tidally_locked_moon_faces_the_planet() {
        let s = system();
        // The moon-surface direction pointing at the planet is constant over time.
        let facing = |t: f64| {
            let planet_in_moon = s
                .tree
                .transform_point(s.moon_surface, s.planet, t, DVec3::ZERO);
            planet_in_moon.normalize()
        };
        let d0 = facing(0.0);
        assert!(
            d0.dot(-DVec3::X) > 0.999_999,
            "local -X faces the planet: {d0}"
        );
        for k in 1..10 {
            assert!(facing(k as f64 * 3_000.0).dot(d0) > 0.999_999);
        }
    }

    #[test]
    fn precise_far_from_the_root() {
        // Two frames 10¹² m from the root, 10 m apart: their relative position must be
        // accurate to well under 1 mm (development plan §7.2, task 1.2).
        let mut tree = FrameTree::new("galaxy");
        let root = tree.root();
        let far = 1.0e12;
        let orbit = KeplerOrbit::circular(1.0e25, far, 0.0, 0.0, 0.0);
        let a = tree.add("a", root, FrameMotion::Orbit(orbit)).unwrap();
        let (pos_a, _) = orbit.state_at(0.0);
        let b = tree
            .add(
                "b",
                root,
                FrameMotion::Fixed {
                    position: pos_a + DVec3::new(10.0, 0.0, 0.0),
                    rotation: DQuat::IDENTITY,
                },
            )
            .unwrap();
        let rel = tree.state_in(a, b, 0.0).position;
        assert!(vec_close(rel, DVec3::new(10.0, 0.0, 0.0), 1e-3), "{rel}");
    }
}
