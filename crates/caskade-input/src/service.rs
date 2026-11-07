//! The running input service: the normalizer on its own thread, fed by the
//! `gilrs` reader and by injected events through one channel. It sleeps
//! until input arrives or a held direction is due to repeat.

use crate::{InputAction, Normalizer, PadInfo, Raw, Timing};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Instant,
};

/// What the service reports to its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Action(InputAction),
    /// A pad connected or disconnected; the pads now connected.
    Devices(Vec<PadInfo>),
}

type Sink = Arc<dyn Fn(InputEvent) + Send + Sync>;

struct Inner {
    sender: Mutex<mpsc::Sender<Raw>>,
    pads: Arc<Mutex<Vec<PadInfo>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Cheap to clone; the threads stop when the last clone is dropped.
#[derive(Clone)]
pub struct InputService {
    inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl InputService {
    /// Reads real pads (with the `gilrs` feature) and injected input. A host
    /// without pad support logs it once and keeps working with injection.
    pub fn start(sink: impl Fn(InputEvent) + Send + Sync + 'static) -> Self {
        let service = Self::spawn(Arc::new(sink), Timing::default());
        #[cfg(feature = "gilrs")]
        service.read_pads();
        service
    }

    /// Injected input only: tests and end-to-end runs drive the app "with a
    /// pad" through the real normalizer, without hardware.
    pub fn without_pads(sink: impl Fn(InputEvent) + Send + Sync + 'static, timing: Timing) -> Self {
        Self::spawn(Arc::new(sink), timing)
    }

    /// The connected pads.
    pub fn devices(&self) -> Vec<PadInfo> {
        lock(&self.inner.pads).clone()
    }

    /// Feeds a raw event as if a pad had sent it.
    pub fn inject(&self, raw: Raw) {
        // The receiver only goes away with the last clone of the service.
        let _ = lock(&self.inner.sender).send(raw);
    }

    fn spawn(sink: Sink, timing: Timing) -> Self {
        let (sender, receiver) = mpsc::channel();
        let pads = Arc::new(Mutex::new(Vec::new()));
        let shared = pads.clone();
        std::thread::Builder::new()
            .name("caskade-input".into())
            .spawn(move || normalize(receiver, Normalizer::new(timing), &sink, &shared))
            .expect("start the input thread");
        Self {
            inner: Arc::new(Inner {
                sender: Mutex::new(sender),
                pads,
                stop: Arc::new(AtomicBool::new(false)),
            }),
        }
    }

    #[cfg(feature = "gilrs")]
    fn read_pads(&self) {
        let sender = lock(&self.inner.sender).clone();
        let stop = self.inner.stop.clone();
        let started = std::thread::Builder::new()
            .name("caskade-gilrs".into())
            .spawn(move || pads::read(&sender, &stop));
        if let Err(error) = started {
            tracing::warn!(%error, "gamepad reader didn't start");
        }
        #[cfg(windows)]
        {
            let sender = lock(&self.inner.sender).clone();
            let stop = self.inner.stop.clone();
            let started = std::thread::Builder::new()
                .name("caskade-guide".into())
                .spawn(move || guide::watch(&sender, &stop));
            if let Err(error) = started {
                tracing::warn!(%error, "guide button reader didn't start");
            }
        }
    }
}

/// The guide button on Windows while another program has focus, which is
/// when gilrs hears nothing (`caskade_xinput`). With Caskade focused, gilrs
/// has the pads.
#[cfg(windows)]
mod guide {
    use crate::{Button, PadId, Raw};
    use caskade_xinput::{PADS, Reader, focused, wants_guide};
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::Duration,
    };

    /// Not a pad gilrs numbers.
    const PAD: PadId = PadId::MAX;
    const POLL: Duration = Duration::from_millis(50);
    /// An empty slot makes XInput look for a device, which is slow enough to
    /// show in a game: those wait this many polls.
    const EMPTY_EVERY: u32 = 40;

    pub(super) fn watch(sender: &mpsc::Sender<Raw>, stop: &AtomicBool) {
        let Some(reader) = Reader::new() else {
            tracing::warn!("this Windows has no XInput: no guide button over games");
            return;
        };
        let mut connected = [false; PADS as usize];
        let mut held = false;
        let mut tick = 0u32;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(POLL);
            tick = tick.wrapping_add(1);
            let wanted = !focused() && {
                let mut wanted = false;
                for index in 0..PADS {
                    let slot = &mut connected[index as usize];
                    if *slot || tick.is_multiple_of(EMPTY_EVERY) {
                        let buttons = reader.buttons(index);
                        *slot = buttons.is_some();
                        wanted |= buttons.is_some_and(wants_guide);
                    }
                }
                wanted
            };
            if wanted != held {
                held = wanted;
                let raw = if held {
                    Raw::Pressed(PAD, Button::Guide)
                } else {
                    Raw::Released(PAD, Button::Guide)
                };
                if sender.send(raw).is_err() {
                    return;
                }
            }
        }
    }
}

/// The normalizer thread: runs until every sender is gone.
fn normalize(
    receiver: mpsc::Receiver<Raw>,
    mut normalizer: Normalizer,
    sink: &Sink,
    pads: &Mutex<Vec<PadInfo>>,
) {
    loop {
        let raw = match normalizer.deadline() {
            Some(due) => match receiver.recv_timeout(due.saturating_duration_since(Instant::now()))
            {
                Ok(raw) => Some(raw),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
            None => match receiver.recv() {
                Ok(raw) => Some(raw),
                Err(_) => break,
            },
        };
        let mut actions = Vec::new();
        let mut devices = false;
        if let Some(raw) = raw {
            devices = matches!(raw, Raw::Connected { .. } | Raw::Disconnected(_));
            actions = normalizer.feed(raw, Instant::now());
        }
        actions.extend(normalizer.tick(Instant::now()));
        if devices || !actions.is_empty() {
            *lock(pads) = normalizer.pads();
        }
        if devices {
            deliver(sink, InputEvent::Devices(normalizer.pads()));
        }
        for action in actions {
            deliver(sink, InputEvent::Action(action));
        }
    }
}

/// Hands one event to the sink. A panic in the sink loses that event, not
/// the thread: pads keep working for the rest of the session.
fn deliver(sink: &Sink, event: InputEvent) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(event)));
    if let Err(panic) = result {
        let message = panic
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("unknown panic");
        tracing::error!(
            panic = message,
            "input event handler panicked; event dropped"
        );
    }
}

#[cfg(feature = "gilrs")]
mod pads {
    use crate::{Button, GlyphFamily, Raw, Stick};
    use gilrs::{Axis, EventType, Gilrs};
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::Duration,
    };

    fn id(id: gilrs::GamepadId) -> u32 {
        u32::try_from(usize::from(id)).unwrap_or(u32::MAX)
    }

    fn button(button: gilrs::Button) -> Option<Button> {
        use gilrs::Button as B;
        Some(match button {
            B::South => Button::South,
            B::East => Button::East,
            B::West => Button::West,
            B::North => Button::North,
            B::LeftTrigger => Button::LeftBumper,
            B::RightTrigger => Button::RightBumper,
            B::LeftTrigger2 => Button::LeftTrigger,
            B::RightTrigger2 => Button::RightTrigger,
            B::Start => Button::Start,
            B::Select => Button::Select,
            B::Mode => Button::Guide,
            B::DPadUp => Button::DPadUp,
            B::DPadDown => Button::DPadDown,
            B::DPadLeft => Button::DPadLeft,
            B::DPadRight => Button::DPadRight,
            _ => return None,
        })
    }

    fn connected(pad: gilrs::Gamepad<'_>) -> Raw {
        let name = pad.name().to_owned();
        Raw::Connected {
            pad: id(pad.id()),
            glyphs: GlyphFamily::detect(pad.vendor_id(), &name),
            name,
        }
    }

    /// Forwards pad events until `stop`; blocks between events.
    pub(super) fn read(sender: &mpsc::Sender<Raw>, stop: &AtomicBool) {
        let mut gilrs = match Gilrs::new() {
            Ok(gilrs) => gilrs,
            Err(error) => {
                tracing::warn!(%error, "no gamepad support on this host; console mode still works with the keyboard");
                return;
            }
        };
        for (_, pad) in gilrs.gamepads() {
            if sender.send(connected(pad)).is_err() {
                return;
            }
        }
        while !stop.load(Ordering::Relaxed) {
            let Some(event) = gilrs.next_event_blocking(Some(Duration::from_secs(1))) else {
                continue;
            };
            let pad = id(event.id);
            let raw = match event.event {
                EventType::ButtonPressed(b, _) => button(b).map(|b| Raw::Pressed(pad, b)),
                EventType::ButtonReleased(b, _) => button(b).map(|b| Raw::Released(pad, b)),
                EventType::AxisChanged(Axis::LeftStickX, value, _) => {
                    Some(Raw::Axis(pad, Stick::X, value))
                }
                EventType::AxisChanged(Axis::LeftStickY, value, _) => {
                    Some(Raw::Axis(pad, Stick::Y, value))
                }
                EventType::Connected => Some(connected(gilrs.gamepad(event.id))),
                EventType::Disconnected => Some(Raw::Disconnected(pad)),
                _ => None,
            };
            if let Some(raw) = raw
                && sender.send(raw).is_err()
            {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Button, GlyphFamily};
    use std::time::Duration;

    /// The real service starts whether or not a pad (or pad support) is
    /// there, and injected input still flows next to it.
    #[test]
    fn the_real_service_starts_with_or_without_pads() {
        let (sender, events) = mpsc::channel();
        let sender = Mutex::new(sender);
        let service = InputService::start(move |event| {
            let _ = lock(&sender).send(event);
        });
        service.inject(Raw::Pressed(999, Button::Start));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match events.recv_timeout(left).expect("the injected press") {
                InputEvent::Action(action) if action.action == Action::Menu => break,
                _ => {}
            }
        }
        assert!(
            service
                .devices()
                .iter()
                .any(|pad| pad.id == 999 && pad.active)
        );
    }

    #[test]
    fn a_panicking_sink_loses_the_event_not_the_thread() {
        let (sender, events) = mpsc::channel();
        let sender = Mutex::new(sender);
        let service = InputService::without_pads(
            move |event| {
                if let InputEvent::Action(action) = &event
                    && action.action == Action::Menu
                {
                    panic!("the sink fails on Start");
                }
                let _ = lock(&sender).send(event);
            },
            Timing::default(),
        );
        service.inject(Raw::Pressed(1, Button::Start));
        service.inject(Raw::Released(1, Button::Start));
        service.inject(Raw::Pressed(1, Button::South));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match events
                .recv_timeout(left)
                .expect("the press after the panic")
            {
                InputEvent::Action(action) if action.action == Action::Confirm => break,
                _ => {}
            }
        }
    }

    #[test]
    fn injected_input_comes_out_as_actions_and_devices() {
        let (sender, events) = mpsc::channel();
        let sender = Mutex::new(sender);
        let service = InputService::without_pads(
            move |event| {
                let _ = lock(&sender).send(event);
            },
            Timing {
                initial_delay: Duration::from_millis(30),
                interval: Duration::from_millis(10),
                ..Timing::default()
            },
        );
        service.inject(Raw::Connected {
            pad: 3,
            name: "DualSense".into(),
            glyphs: GlyphFamily::Playstation,
        });
        let next = || events.recv_timeout(Duration::from_secs(5)).unwrap();
        let InputEvent::Devices(pads) = next() else {
            panic!("expected the device list");
        };
        assert_eq!(pads[0].name, "DualSense");

        service.inject(Raw::Pressed(3, Button::DPadLeft));
        let InputEvent::Action(first) = next() else {
            panic!("expected an action");
        };
        assert_eq!((first.action, first.repeat), (Action::NavLeft, false));
        assert_eq!(first.glyphs, GlyphFamily::Playstation);
        // The thread wakes up for repeats on its own.
        let InputEvent::Action(repeat) = next() else {
            panic!("expected a repeat");
        };
        assert!(repeat.repeat);
        service.inject(Raw::Released(3, Button::DPadLeft));
        assert!(service.devices()[0].active);
    }
}
