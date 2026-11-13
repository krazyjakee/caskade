//! Raw pad events to [`Action`]s: button layouts, the stick's deadzone and
//! held directions repeating. Pure: the caller passes the time.

use crate::{Action, GlyphFamily, InputAction, PadInfo, Source};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub type PadId = u32;

/// Buttons by position (South is the bottom face button on every pad).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Start,
    Select,
    Guide,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

/// The left stick's axes; `Y` is positive upwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stick {
    X,
    Y,
}

/// What a backend reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Raw {
    Connected {
        pad: PadId,
        name: String,
        glyphs: GlyphFamily,
    },
    Disconnected(PadId),
    Pressed(PadId, Button),
    Released(PadId, Button),
    /// A left stick axis moved, in -1..=1.
    Axis(PadId, Stick, f32),
}

/// Repeat timing and the stick's deadzone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// From the first action of a held direction to its first repeat.
    pub initial_delay: Duration,
    /// Between repeats.
    pub interval: Duration,
    /// The stick counts as a direction from this far out...
    pub engage: f32,
    /// ...until it comes back inside this (hysteresis).
    pub release: f32,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(400),
            interval: Duration::from_millis(90),
            engage: 0.6,
            release: 0.4,
        }
    }
}

/// Where a held direction comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    DPad(Button),
    Stick,
}

#[derive(Debug)]
struct Pad {
    name: String,
    glyphs: GlyphFamily,
    /// Held directions, the most recent last.
    held: Vec<(Action, Origin)>,
    x: f32,
    y: f32,
    /// Select and Start held, for the guide chord.
    select: bool,
    start: bool,
}

#[derive(Debug, Clone, Copy)]
struct Repeat {
    pad: PadId,
    action: Action,
    next: Instant,
}

#[derive(Debug, Default)]
pub struct Normalizer {
    timing: Timing,
    pads: BTreeMap<PadId, Pad>,
    active: Option<PadId>,
    repeat: Option<Repeat>,
}

impl Normalizer {
    pub fn new(timing: Timing) -> Self {
        Self {
            timing,
            ..Self::default()
        }
    }

    /// Actions for one raw event.
    pub fn feed(&mut self, raw: Raw, now: Instant) -> Vec<InputAction> {
        match raw {
            Raw::Connected { pad, name, glyphs } => {
                self.pads.insert(pad, Pad::new(name, glyphs));
                Vec::new()
            }
            Raw::Disconnected(pad) => {
                self.pads.remove(&pad);
                if self.repeat.is_some_and(|repeat| repeat.pad == pad) {
                    self.repeat = None;
                }
                if self.active == Some(pad) {
                    self.active = None;
                }
                Vec::new()
            }
            Raw::Pressed(pad, button) => {
                let state = self.pad(pad);
                let glyphs = state.glyphs;
                let other_held = match button {
                    Button::Select => std::mem::replace(&mut state.select, true) || state.start,
                    Button::Start => std::mem::replace(&mut state.start, true) || state.select,
                    _ => false,
                };
                // Select and Start pressed together count as `guide`: Steam
                // Input (Steam Deck game mode) keeps the real guide button
                // for itself.
                if other_held {
                    return vec![self.fire(pad, Action::Guide, false)];
                }
                match direction(button) {
                    Some(action) => self.hold(pad, action, Origin::DPad(button), now),
                    None => match face(button, glyphs) {
                        Some(action) => vec![self.fire(pad, action, false)],
                        None => Vec::new(),
                    },
                }
            }
            Raw::Released(pad, button) => {
                if let Some(state) = self.pads.get_mut(&pad) {
                    match button {
                        Button::Select => state.select = false,
                        Button::Start => state.start = false,
                        _ => {}
                    }
                }
                if direction(button).is_some() {
                    self.let_go(pad, Origin::DPad(button), now);
                }
                Vec::new()
            }
            Raw::Axis(pad, stick, value) => self.stick(pad, stick, value, now),
        }
    }

    /// Repeats that are due at `now`.
    pub fn tick(&mut self, now: Instant) -> Vec<InputAction> {
        let Some(repeat) = self.repeat else {
            return Vec::new();
        };
        if now < repeat.next {
            return Vec::new();
        }
        // A late tick fires once, not a burst of missed repeats.
        let next = (repeat.next + self.timing.interval).max(now + self.timing.interval);
        self.repeat = Some(Repeat { next, ..repeat });
        vec![self.fire(repeat.pad, repeat.action, true)]
    }

    /// When [`Self::tick`] next has something to do.
    pub fn deadline(&self) -> Option<Instant> {
        self.repeat.map(|repeat| repeat.next)
    }

    /// The connected pads; the one used last is active.
    pub fn pads(&self) -> Vec<PadInfo> {
        self.pads
            .iter()
            .map(|(id, pad)| PadInfo {
                id: *id,
                name: pad.name.clone(),
                glyphs: pad.glyphs,
                active: self.active == Some(*id),
            })
            .collect()
    }

    /// A pad that sends input before it was announced is still a pad.
    fn pad(&mut self, pad: PadId) -> &mut Pad {
        self.pads
            .entry(pad)
            .or_insert_with(|| Pad::new(format!("Gamepad {pad}"), GlyphFamily::Generic))
    }

    fn fire(&mut self, pad: PadId, action: Action, repeat: bool) -> InputAction {
        self.active = Some(pad);
        InputAction {
            action,
            source: Source::Pad,
            repeat,
            glyphs: self
                .pads
                .get(&pad)
                .map(|pad| pad.glyphs)
                .unwrap_or_default(),
        }
    }

    fn hold(
        &mut self,
        pad: PadId,
        action: Action,
        origin: Origin,
        now: Instant,
    ) -> Vec<InputAction> {
        let held = &mut self.pad(pad).held;
        held.retain(|(_, from)| *from != origin);
        held.push((action, origin));
        self.repeat = Some(Repeat {
            pad,
            action,
            next: now + self.timing.initial_delay,
        });
        vec![self.fire(pad, action, false)]
    }

    /// Releasing the newest direction falls back to one still held, which
    /// repeats again after the initial delay without firing at once.
    fn let_go(&mut self, pad: PadId, origin: Origin, now: Instant) {
        let Some(state) = self.pads.get_mut(&pad) else {
            return;
        };
        state.held.retain(|(_, from)| *from != origin);
        if self.repeat.is_none_or(|repeat| repeat.pad != pad) {
            return;
        }
        self.repeat = state.held.last().map(|(action, _)| Repeat {
            pad,
            action: *action,
            next: now + self.timing.initial_delay,
        });
    }

    fn stick(&mut self, pad: PadId, stick: Stick, value: f32, now: Instant) -> Vec<InputAction> {
        let timing = self.timing;
        let state = self.pad(pad);
        let value = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        match stick {
            Stick::X => state.x = value,
            Stick::Y => state.y = value,
        }
        let (x, y) = (state.x, state.y);
        let current = state
            .held
            .iter()
            .find(|(_, from)| *from == Origin::Stick)
            .map(|(action, _)| *action);
        let magnitude = x.hypot(y);
        let threshold = if current.is_some() {
            timing.release
        } else {
            timing.engage
        };
        let wanted = (magnitude >= threshold).then(|| {
            if x.abs() >= y.abs() {
                if x > 0.0 {
                    Action::NavRight
                } else {
                    Action::NavLeft
                }
            } else if y > 0.0 {
                Action::NavUp
            } else {
                Action::NavDown
            }
        });
        match (current, wanted) {
            (Some(was), Some(now_wanted)) if was == now_wanted => Vec::new(),
            (_, Some(action)) => self.hold(pad, action, Origin::Stick, now),
            (Some(_), None) => {
                self.let_go(pad, Origin::Stick, now);
                Vec::new()
            }
            (None, None) => Vec::new(),
        }
    }
}

impl Pad {
    fn new(name: String, glyphs: GlyphFamily) -> Self {
        Self {
            name,
            glyphs,
            held: Vec::new(),
            x: 0.0,
            y: 0.0,
            select: false,
            start: false,
        }
    }
}

fn direction(button: Button) -> Option<Action> {
    Some(match button {
        Button::DPadUp => Action::NavUp,
        Button::DPadDown => Action::NavDown,
        Button::DPadLeft => Action::NavLeft,
        Button::DPadRight => Action::NavRight,
        _ => return None,
    })
}

/// Nintendo pads confirm with the right face button (A) and go back with
/// the bottom one (B); their X is on top and Y on the left.
fn face(button: Button, glyphs: GlyphFamily) -> Option<Action> {
    let nintendo = glyphs == GlyphFamily::Nintendo;
    Some(match button {
        Button::South if nintendo => Action::Back,
        Button::East if nintendo => Action::Confirm,
        Button::North if nintendo => Action::Details,
        Button::West if nintendo => Action::Search,
        Button::South => Action::Confirm,
        Button::East => Action::Back,
        Button::West => Action::Details,
        Button::North => Action::Search,
        Button::LeftBumper => Action::TabPrev,
        Button::RightBumper => Action::TabNext,
        Button::LeftTrigger => Action::PagePrev,
        Button::RightTrigger => Action::PageNext,
        Button::Start => Action::Menu,
        Button::Select => Action::View,
        Button::Guide => Action::Guide,
        Button::DPadUp | Button::DPadDown | Button::DPadLeft | Button::DPadRight => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    struct Run {
        input: Normalizer,
        start: Instant,
    }

    impl Run {
        fn new() -> Self {
            let mut input = Normalizer::new(Timing::default());
            let start = Instant::now();
            input.feed(
                Raw::Connected {
                    pad: 1,
                    name: "Xbox Wireless Controller".into(),
                    glyphs: GlyphFamily::Xbox,
                },
                start,
            );
            Self { input, start }
        }
        fn at(&self, ms: u64) -> Instant {
            self.start + MS * ms as u32
        }
        /// Feeds `raw` at `ms` and returns the actions' names.
        fn feed(&mut self, ms: u64, raw: Raw) -> Vec<(Action, bool)> {
            let now = self.at(ms);
            names(self.input.feed(raw, now))
        }
        /// Ticks every millisecond from `from` to `to`.
        fn ticks(&mut self, from: u64, to: u64) -> Vec<(u64, Action)> {
            let mut fired = Vec::new();
            for ms in from..=to {
                let now = self.at(ms);
                for action in self.input.tick(now) {
                    assert!(action.repeat);
                    fired.push((ms, action.action));
                }
            }
            fired
        }
    }

    fn names(actions: Vec<InputAction>) -> Vec<(Action, bool)> {
        actions.into_iter().map(|a| (a.action, a.repeat)).collect()
    }

    #[test]
    fn face_buttons_shoulders_and_menu_buttons_map_to_actions() {
        let mut run = Run::new();
        let cases = [
            (Button::South, Action::Confirm),
            (Button::East, Action::Back),
            (Button::West, Action::Details),
            (Button::North, Action::Search),
            (Button::LeftBumper, Action::TabPrev),
            (Button::RightBumper, Action::TabNext),
            (Button::LeftTrigger, Action::PagePrev),
            (Button::RightTrigger, Action::PageNext),
            (Button::Start, Action::Menu),
            (Button::Select, Action::View),
            (Button::Guide, Action::Guide),
        ];
        for (button, action) in cases {
            assert_eq!(run.feed(0, Raw::Pressed(1, button)), [(action, false)]);
            assert!(run.feed(1, Raw::Released(1, button)).is_empty());
        }
        // Only directions repeat.
        run.feed(0, Raw::Pressed(1, Button::South));
        assert!(run.ticks(0, 2000).is_empty());
        assert_eq!(run.input.deadline(), None);
    }

    #[test]
    fn select_and_start_together_are_guide() {
        let mut run = Run::new();
        assert_eq!(
            run.feed(0, Raw::Pressed(1, Button::Select)),
            [(Action::View, false)]
        );
        assert_eq!(
            run.feed(10, Raw::Pressed(1, Button::Start)),
            [(Action::Guide, false)]
        );
        run.feed(20, Raw::Released(1, Button::Select));
        run.feed(20, Raw::Released(1, Button::Start));
        // Apart, they are themselves again.
        assert_eq!(
            run.feed(30, Raw::Pressed(1, Button::Start)),
            [(Action::Menu, false)]
        );
    }

    #[test]
    fn nintendo_pads_swap_confirm_and_back() {
        let mut run = Run::new();
        run.feed(
            0,
            Raw::Connected {
                pad: 2,
                name: "Pro Controller".into(),
                glyphs: GlyphFamily::Nintendo,
            },
        );
        let pressed = |run: &mut Run, button| run.feed(0, Raw::Pressed(2, button))[0].0;
        assert_eq!(pressed(&mut run, Button::East), Action::Confirm);
        assert_eq!(pressed(&mut run, Button::South), Action::Back);
        assert_eq!(pressed(&mut run, Button::North), Action::Details);
        assert_eq!(pressed(&mut run, Button::West), Action::Search);
        let glyphs = run.input.feed(Raw::Pressed(2, Button::Start), run.at(0))[0].glyphs;
        assert_eq!(glyphs, GlyphFamily::Nintendo);
    }

    #[test]
    fn a_held_direction_repeats_after_the_delay_until_released() {
        let mut run = Run::new();
        assert_eq!(
            run.feed(0, Raw::Pressed(1, Button::DPadDown)),
            [(Action::NavDown, false)]
        );
        assert_eq!(run.input.deadline(), Some(run.at(400)));
        let fired = run.ticks(1, 700);
        let times: Vec<u64> = fired.iter().map(|(ms, _)| *ms).collect();
        assert_eq!(times, [400, 490, 580, 670]);
        assert!(fired.iter().all(|(_, action)| *action == Action::NavDown));
        run.feed(700, Raw::Released(1, Button::DPadDown));
        assert!(run.ticks(701, 2000).is_empty());
    }

    #[test]
    fn a_late_tick_fires_once_instead_of_a_burst() {
        let mut run = Run::new();
        run.feed(0, Raw::Pressed(1, Button::DPadRight));
        let now = run.at(5000);
        assert_eq!(run.input.tick(now).len(), 1);
        assert!(run.input.tick(now).is_empty());
        assert_eq!(run.input.deadline(), Some(run.at(5090)));
        assert_eq!(run.input.tick(run.at(5090)).len(), 1);
    }

    #[test]
    fn the_stick_has_a_deadzone_with_hysteresis() {
        let mut run = Run::new();
        // Inside the deadzone: nothing.
        assert!(run.feed(0, Raw::Axis(1, Stick::X, 0.5)).is_empty());
        // Out past `engage`: one action, then repeats.
        assert_eq!(
            run.feed(10, Raw::Axis(1, Stick::X, 0.7)),
            [(Action::NavRight, false)]
        );
        // Drifting back to 0.5 keeps holding (above `release`).
        assert!(run.feed(20, Raw::Axis(1, Stick::X, 0.5)).is_empty());
        assert_eq!(run.ticks(21, 410), [(410, Action::NavRight)]);
        // Inside `release`: let go.
        assert!(run.feed(420, Raw::Axis(1, Stick::X, 0.3)).is_empty());
        assert!(run.ticks(421, 2000).is_empty());
        // Up is positive Y; the dominant axis wins.
        assert_eq!(
            run.feed(2000, Raw::Axis(1, Stick::Y, 0.9)),
            [(Action::NavUp, false)]
        );
        // Swinging to another direction fires that one straight away.
        run.feed(2010, Raw::Axis(1, Stick::Y, 0.0));
        assert_eq!(
            run.feed(2010, Raw::Axis(1, Stick::X, -0.8)),
            [(Action::NavLeft, false)]
        );
        // A bad value counts as centred.
        run.feed(2020, Raw::Axis(1, Stick::X, f32::NAN));
        assert!(run.ticks(2021, 3000).is_empty());
    }

    #[test]
    fn stick_and_dpad_together_follow_the_newest_and_fall_back() {
        let mut run = Run::new();
        assert_eq!(
            run.feed(0, Raw::Pressed(1, Button::DPadRight)),
            [(Action::NavRight, false)]
        );
        assert_eq!(
            run.feed(100, Raw::Axis(1, Stick::Y, -1.0)),
            [(Action::NavDown, false)]
        );
        assert_eq!(run.ticks(101, 500), [(500, Action::NavDown)]);
        // Releasing the stick falls back to the D-pad, which still repeats
        // after a fresh delay but doesn't fire at once.
        assert!(run.feed(550, Raw::Axis(1, Stick::Y, 0.0)).is_empty());
        assert_eq!(run.ticks(551, 950), [(950, Action::NavRight)]);
        run.feed(960, Raw::Released(1, Button::DPadRight));
        assert!(run.ticks(961, 3000).is_empty());
    }

    #[test]
    fn a_pad_disconnecting_mid_hold_stops_its_repeats() {
        let mut run = Run::new();
        run.feed(0, Raw::Pressed(1, Button::DPadUp));
        assert!(run.input.pads()[0].active);
        run.feed(100, Raw::Disconnected(1));
        assert!(run.ticks(101, 3000).is_empty());
        assert!(run.input.pads().is_empty());
        // A release after the disconnect is harmless.
        assert!(run.feed(200, Raw::Released(1, Button::DPadUp)).is_empty());
    }

    #[test]
    fn the_last_pad_used_is_active_and_unknown_pads_are_generic() {
        let mut run = Run::new();
        let action = run.input.feed(Raw::Pressed(7, Button::South), run.at(0));
        assert_eq!(action[0].glyphs, GlyphFamily::Generic);
        let pads = run.input.pads();
        assert_eq!(pads.len(), 2);
        assert!(pads.iter().any(|pad| pad.id == 7 && pad.active));
        assert!(pads.iter().any(|pad| pad.id == 1 && !pad.active));
    }

    #[test]
    fn glyph_families_come_from_vendor_ids_then_names() {
        assert_eq!(
            GlyphFamily::detect(Some(0x054c), "x"),
            GlyphFamily::Playstation
        );
        assert_eq!(
            GlyphFamily::detect(Some(0x057e), "x"),
            GlyphFamily::Nintendo
        );
        assert_eq!(
            GlyphFamily::detect(Some(0x28de), "Steam Deck"),
            GlyphFamily::Xbox
        );
        assert_eq!(
            GlyphFamily::detect(None, "DualSense Wireless Controller"),
            GlyphFamily::Playstation
        );
        assert_eq!(
            GlyphFamily::detect(None, "8BitDo Pro 2"),
            GlyphFamily::Generic
        );
    }

    #[test]
    fn action_names_are_the_dotted_contract() {
        let names: Vec<String> = [
            Action::NavUp,
            Action::Confirm,
            Action::TabNext,
            Action::Guide,
        ]
        .iter()
        .map(|action| serde_json::to_string(action).unwrap())
        .collect();
        assert_eq!(
            names,
            ["\"nav.up\"", "\"confirm\"", "\"tab.next\"", "\"guide\""]
        );
    }
}
