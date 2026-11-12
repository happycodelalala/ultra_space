//! Scripted input for automated test runs (`ULTRA_SPACE_SCRIPT=<file>`).
//!
//! A script is a text file of timed actions, replayed like a recording so a test flight can be
//! repeated exactly (typically on the native Windows build, launched from WSL by
//! `tools/run-windows.sh`). One action per line; `#` starts a comment; times are seconds after
//! the world is shown (after loading and the warm-up):
//!
//! ```text
//! 1.0  press N            # tap a key for one frame
//! 2.0  hold W 5           # hold a key (or MouseLeft / MouseRight) for 5 s
//! 3.0  mouse 0 8 2        # move the mouse by (0, 8) px every frame for 2 s
//! 9.0  shot descent.png   # screenshot, relative to the script's folder
//! 9.5  log reached cloud deck
//! 10   wait landed 20     # pause the script clock until the ship is landed and still (max 20 s)
//! 12   quit               # exit (after pending screenshots), logging a frame summary
//! ```
//!
//! Scripted input goes through Bevy's input state, so the game sees it like real input. While a
//! script runs the cursor isn't grabbed and losing window focus doesn't release the mouse, so
//! someone can watch the window without it taking over their desktop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::input::InputSystems;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use sim_vehicle::FlightTelemetry;

use crate::ship::PlayerShip;
use crate::sim::Sim;
use crate::universe::GameState;

/// Hitch budget for the summary (overview §14.6).
const HITCH_BUDGET: Duration = Duration::from_millis(50);
/// Longest wait for pending screenshots before quitting anyway.
const SHOT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Press(Input),
    Hold(Input, f64),
    Mouse(Vec2, f64),
    Shot(String),
    Log(String),
    /// Pause the script clock until a condition holds (at most this many seconds).
    Wait(Condition, f64),
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Condition {
    /// The player's ship touches the ground and is (nearly) still relative to it.
    Landed,
}

/// Default limit for `wait` (s).
const WAIT_TIMEOUT: f64 = 20.0;
/// "Still" for `wait landed` (m/s relative to the ground).
const LANDED_SPEED: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub at: f64,
    pub action: Action,
}

/// Marks that a script is running (other systems relax mouse handling).
#[derive(Resource)]
pub struct ScriptRunning;

#[derive(Resource)]
struct Script {
    steps: Vec<Step>,
    next: usize,
    dir: PathBuf,
    started: Option<Duration>,
    holds: Vec<(Input, f64)>,
    taps: Vec<Input>,
    moves: Vec<(Vec2, f64)>,
    shots: Vec<PathBuf>,
    quit_at: Option<Duration>,
    /// An active `wait`: condition, real time it started, limit (s).
    waiting: Option<(Condition, Duration, f64)>,
    frames: u64,
    worst: Duration,
    over_budget: u64,
    total: Duration,
}

/// Parses a key or mouse button name: letters, digits, `Space`, `Ctrl`, `Shift`, `Tab`, `Esc`,
/// `F1`–`F12`, `Comma`, `Period`, `BracketLeft`, `BracketRight`, `Enter`, `MouseLeft`,
/// `MouseRight`.
pub fn parse_input(name: &str) -> Option<Input> {
    let key = |k| Some(Input::Key(k));
    let upper = name.to_ascii_uppercase();
    if upper.len() == 1 {
        let c = upper.chars().next()?;
        const LETTERS: [KeyCode; 26] = [
            KeyCode::KeyA,
            KeyCode::KeyB,
            KeyCode::KeyC,
            KeyCode::KeyD,
            KeyCode::KeyE,
            KeyCode::KeyF,
            KeyCode::KeyG,
            KeyCode::KeyH,
            KeyCode::KeyI,
            KeyCode::KeyJ,
            KeyCode::KeyK,
            KeyCode::KeyL,
            KeyCode::KeyM,
            KeyCode::KeyN,
            KeyCode::KeyO,
            KeyCode::KeyP,
            KeyCode::KeyQ,
            KeyCode::KeyR,
            KeyCode::KeyS,
            KeyCode::KeyT,
            KeyCode::KeyU,
            KeyCode::KeyV,
            KeyCode::KeyW,
            KeyCode::KeyX,
            KeyCode::KeyY,
            KeyCode::KeyZ,
        ];
        const DIGITS: [KeyCode; 10] = [
            KeyCode::Digit0,
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
        ];
        return match c {
            'A'..='Z' => key(LETTERS[(c as u8 - b'A') as usize]),
            '0'..='9' => key(DIGITS[(c as u8 - b'0') as usize]),
            _ => None,
        };
    }
    const F_KEYS: [KeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];
    if let Some(n) = upper
        .strip_prefix('F')
        .and_then(|n| n.parse::<usize>().ok())
    {
        return F_KEYS.get(n.checked_sub(1)?).copied().map(Input::Key);
    }
    match upper.as_str() {
        "SPACE" => key(KeyCode::Space),
        "CTRL" | "CONTROL" | "CONTROLLEFT" => key(KeyCode::ControlLeft),
        "SHIFT" | "SHIFTLEFT" => key(KeyCode::ShiftLeft),
        "TAB" => key(KeyCode::Tab),
        "ESC" | "ESCAPE" => key(KeyCode::Escape),
        "ENTER" => key(KeyCode::Enter),
        "COMMA" => key(KeyCode::Comma),
        "PERIOD" => key(KeyCode::Period),
        "BRACKETLEFT" => key(KeyCode::BracketLeft),
        "BRACKETRIGHT" => key(KeyCode::BracketRight),
        "MOUSELEFT" => Some(Input::Mouse(MouseButton::Left)),
        "MOUSERIGHT" => Some(Input::Mouse(MouseButton::Right)),
        _ => None,
    }
}

/// Parses a script; errors name the line.
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let err = |msg: &str| format!("script line {}: {msg}: {raw:?}", n + 1);
        let mut words = line.split_whitespace();
        let at: f64 = words
            .next()
            .and_then(|w| w.parse().ok())
            .ok_or_else(|| err("expected a time in seconds"))?;
        let verb = words.next().ok_or_else(|| err("expected an action"))?;
        let rest: Vec<&str> = words.collect();
        let number = |i: usize| -> Result<f64, String> {
            rest.get(i)
                .and_then(|w| w.parse().ok())
                .ok_or_else(|| err("expected a number"))
        };
        let input = || -> Result<Input, String> {
            rest.first()
                .and_then(|w| parse_input(w))
                .ok_or_else(|| err("unknown key"))
        };
        let action = match verb.to_ascii_lowercase().as_str() {
            "press" => Action::Press(input()?),
            "hold" => Action::Hold(input()?, number(1)?),
            "mouse" => Action::Mouse(Vec2::new(number(0)? as f32, number(1)? as f32), number(2)?),
            "shot" => Action::Shot(
                rest.first()
                    .ok_or_else(|| err("expected a file name"))?
                    .to_string(),
            ),
            "log" => Action::Log(rest.join(" ")),
            "wait" => {
                let condition = match rest.first().map(|w| w.to_ascii_lowercase()).as_deref() {
                    Some("landed") => Condition::Landed,
                    _ => return Err(err("unknown condition (expected: landed)")),
                };
                let limit = if rest.len() > 1 {
                    number(1)?
                } else {
                    WAIT_TIMEOUT
                };
                Action::Wait(condition, limit)
            }
            "quit" => Action::Quit,
            _ => return Err(err("unknown action")),
        };
        steps.push(Step { at, action });
    }
    steps.sort_by(|a, b| a.at.total_cmp(&b.at));
    Ok(steps)
}

pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        let Some(path) = std::env::var_os("ULTRA_SPACE_SCRIPT").map(PathBuf::from) else {
            return;
        };
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read script {}: {e}", path.display()));
        let steps = parse(&text).unwrap_or_else(|e| panic!("{e}"));
        info!("script: {} steps from {}", steps.len(), path.display());
        app.insert_resource(ScriptRunning)
            .insert_resource(Script {
                steps,
                next: 0,
                dir: path.parent().map_or_else(PathBuf::new, Path::to_path_buf),
                started: None,
                holds: Vec::new(),
                taps: Vec::new(),
                moves: Vec::new(),
                shots: Vec::new(),
                quit_at: None,
                waiting: None,
                frames: 0,
                worst: Duration::ZERO,
                over_budget: 0,
                total: Duration::ZERO,
            })
            .add_systems(
                PreUpdate,
                run_script
                    .after(InputSystems)
                    .run_if(in_state(GameState::Running))
                    .run_if(resource_exists::<crate::warmup::WarmupDone>),
            );
    }
}

fn set(
    input: &Input,
    down: bool,
    keys: &mut ButtonInput<KeyCode>,
    buttons: &mut ButtonInput<MouseButton>,
) {
    match (input, down) {
        (Input::Key(k), true) => keys.press(*k),
        (Input::Key(k), false) => keys.release(*k),
        (Input::Mouse(b), true) => buttons.press(*b),
        (Input::Mouse(b), false) => buttons.release(*b),
    }
}

/// Whether a `wait` condition holds now.
fn holds(condition: Condition, sim: &Sim, ship: Option<&PlayerShip>) -> bool {
    match condition {
        Condition::Landed => {
            let sim = &sim.0;
            ship.and_then(|ship| sim.objects.location(ship.id))
                .and_then(|loc| {
                    sim.regions
                        .get(loc.region)?
                        .world
                        .get::<FlightTelemetry>(loc.entity)
                        .map(|t| t.in_contact && t.relative_velocity.length() < LANDED_SPEED)
                })
                .unwrap_or(false)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    mut commands: Commands,
    sim: Res<Sim>,
    ship: Option<Res<PlayerShip>>,
    mut script: ResMut<Script>,
    real: Res<Time<Real>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut motion: ResMut<AccumulatedMouseMotion>,
) {
    let script = &mut *script;
    let now = real.elapsed();
    let started = *script.started.get_or_insert(now);

    // While waiting, the script clock stands still (holds and moves keep their timing).
    if let Some((condition, since, limit)) = script.waiting {
        let waited = (now - since).as_secs_f64();
        let met = holds(condition, &sim, ship.as_deref());
        if met || waited > limit {
            if met {
                info!("script: waited {waited:.1} s for {condition:?}");
            } else {
                error!("script: gave up waiting for {condition:?} after {limit:.0} s");
            }
            script.waiting = None;
        }
        script.started = Some(started + real.delta());
    }
    let started = script.started.unwrap_or(started);
    let t = (now - started).as_secs_f64();

    // Frame statistics while the script runs.
    let dt = real.delta();
    script.frames += 1;
    script.total += dt;
    script.worst = script.worst.max(dt);
    script.over_budget += u64::from(dt > HITCH_BUDGET);

    // Taps last one frame.
    for input in std::mem::take(&mut script.taps) {
        set(&input, false, &mut keys, &mut buttons);
    }

    while script.waiting.is_none()
        && script.next < script.steps.len()
        && script.steps[script.next].at <= t
    {
        let step = script.steps[script.next].clone();
        script.next += 1;
        match step.action {
            Action::Press(input) => {
                set(&input, true, &mut keys, &mut buttons);
                script.taps.push(input);
            }
            Action::Hold(input, seconds) => script.holds.push((input, t + seconds)),
            Action::Mouse(delta, seconds) => script.moves.push((delta, t + seconds)),
            Action::Shot(name) => {
                let path = script.dir.join(&name);
                let _ = std::fs::remove_file(&path);
                info!("script: screenshot {}", path.display());
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path.clone()));
                script.shots.push(path);
            }
            Action::Log(text) => info!("script: {text}"),
            Action::Wait(condition, limit) => script.waiting = Some((condition, now, limit)),
            Action::Quit => script.quit_at = Some(now),
        }
    }

    // Holds: pressed while active, released when done.
    let (active, done): (Vec<_>, Vec<_>) = script.holds.drain(..).partition(|(_, end)| t < *end);
    for (input, _) in &done {
        set(input, false, &mut keys, &mut buttons);
    }
    for (input, _) in &active {
        if !match input {
            Input::Key(k) => keys.pressed(*k),
            Input::Mouse(b) => buttons.pressed(*b),
        } {
            set(input, true, &mut keys, &mut buttons);
        }
    }
    script.holds = active;

    script.moves.retain(|(_, end)| t < *end);
    for (delta, _) in &script.moves {
        motion.delta += *delta;
    }

    if let Some(quit_at) = script.quit_at {
        let shots_done = script.shots.iter().all(|p| p.exists());
        if shots_done || now - quit_at > SHOT_TIMEOUT {
            let avg = script.total.as_secs_f64() / script.frames.max(1) as f64;
            info!(
                "script: done after {:.1} s: {} frames, average {:.1} ms ({:.0} fps), worst {:.1} ms, {} frames over {} ms",
                t,
                script.frames,
                avg * 1e3,
                1.0 / avg.max(1e-9),
                script.worst.as_secs_f64() * 1e3,
                script.over_budget,
                HITCH_BUDGET.as_millis()
            );
            if !shots_done {
                error!("script: some screenshots were not written");
            }
            commands.write_message(AppExit::Success);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_script() {
        let steps = parse(
            "# test flight\n\
             2.0 hold W 5\n\
             1   press n   # arcade\n\
             3 mouse 0 -8 2\n\
             9 shot out.png\n\
             9.5 log hello world\n\
             10 wait landed 15\n\
             12 quit\n",
        )
        .unwrap();
        assert_eq!(steps.len(), 7);
        assert_eq!(steps[0].action, Action::Press(Input::Key(KeyCode::KeyN)));
        assert_eq!(
            steps[1].action,
            Action::Hold(Input::Key(KeyCode::KeyW), 5.0)
        );
        assert_eq!(steps[2].action, Action::Mouse(Vec2::new(0.0, -8.0), 2.0));
        assert_eq!(steps[3].action, Action::Shot("out.png".into()));
        assert_eq!(steps[4].action, Action::Log("hello world".into()));
        assert_eq!(steps[5].action, Action::Wait(Condition::Landed, 15.0));
        assert_eq!(steps[6].action, Action::Quit);
        assert_eq!(
            parse("1 wait landed").unwrap()[0].action,
            Action::Wait(Condition::Landed, WAIT_TIMEOUT)
        );
        assert!(parse("1 wait docked").is_err());
    }

    #[test]
    fn names_keys() {
        assert_eq!(parse_input("Space"), Some(Input::Key(KeyCode::Space)));
        assert_eq!(parse_input("3"), Some(Input::Key(KeyCode::Digit3)));
        assert_eq!(parse_input("f4"), Some(Input::Key(KeyCode::F4)));
        assert_eq!(
            parse_input("MouseRight"),
            Some(Input::Mouse(MouseButton::Right))
        );
        assert_eq!(parse_input("nope"), None);
    }

    #[test]
    fn reports_bad_lines() {
        let e = parse("1 fly W").unwrap_err();
        assert!(e.contains("line 1") && e.contains("unknown action"), "{e}");
        assert!(parse("x press W").is_err());
        assert!(parse("1 hold W").is_err());
    }
}
