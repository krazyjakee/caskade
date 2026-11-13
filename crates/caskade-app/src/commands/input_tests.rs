//! #146: an injected pad press reaches the UI as an input action, only
//! while Caskade has focus (except `guide`).
use super::*;
use caskade_input::{Button, GlyphFamily, Raw, Timing};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// The input actions that reached the UI.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<InputAction>>>);

impl crate::events::Emit for Sink {
    fn emit(&self, event: AppEvent) -> Result<(), String> {
        if let AppEvent::InputActionEvent(InputActionEvent(action)) = event {
            self.0.lock().unwrap().push(action);
        }
        Ok(())
    }
}

#[test]
fn injected_pad_input_reaches_the_ui_while_focused() {
    let bus = Bus::default();
    let mut bus_events = bus.stream();
    let focused = Arc::new(AtomicBool::new(true));
    let summoned = Arc::new(AtomicUsize::new(0));
    let ui = Sink::default();
    let service = InputService::without_pads(
        sink(
            Events::new(ui.clone()),
            Some(bus),
            {
                let focused = focused.clone();
                move || focused.load(Ordering::SeqCst)
            },
            {
                let summoned = summoned.clone();
                move || {
                    summoned.fetch_add(1, Ordering::SeqCst);
                }
            },
        ),
        Timing::default(),
    );
    let received = ui.0;
    let wait_for = |count: usize| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while received.lock().unwrap().len() < count {
            assert!(Instant::now() < deadline, "no input action arrived");
            std::thread::sleep(Duration::from_millis(5));
        }
    };

    service.inject(Raw::Connected {
        pad: 1,
        name: "Xbox Wireless Controller".into(),
        glyphs: GlyphFamily::Xbox,
    });
    service.inject(Raw::Pressed(1, Button::South));
    wait_for(1);
    let first = received.lock().unwrap()[0];
    assert_eq!(first.action, Action::Confirm);
    assert_eq!(first.glyphs, GlyphFamily::Xbox);
    assert_eq!(first.source, caskade_input::Source::Pad);
    assert!(matches!(
        bus_events.try_recv(),
        Ok(Event::InputDevicesChanged { connected: 1 })
    ));
    let devices = input_devices(&service);
    assert_eq!(devices[0].name, "Xbox Wireless Controller");
    assert!(devices[0].active);

    // Unfocused: only `guide` gets through.
    focused.store(false, Ordering::SeqCst);
    service.inject(Raw::Pressed(1, Button::East));
    service.inject(Raw::Pressed(1, Button::Guide));
    wait_for(2);
    std::thread::sleep(Duration::from_millis(50));
    let actions: Vec<Action> = received.lock().unwrap().iter().map(|a| a.action).collect();
    assert_eq!(actions, [Action::Confirm, Action::Guide]);
    // #149: guide summons Caskade even while it isn't focused.
    assert_eq!(summoned.load(Ordering::SeqCst), 1);
    // Select + Start is guide too (Steam Input keeps the real button).
    service.inject(Raw::Pressed(1, Button::Select));
    service.inject(Raw::Pressed(1, Button::Start));
    wait_for(3);
    assert_eq!(received.lock().unwrap()[2].action, Action::Guide);
    assert_eq!(summoned.load(Ordering::SeqCst), 2);
}

#[test]
fn repeated_guide_presses_raise_the_window_once() {
    let summoner = Summoner::default();
    let start = Instant::now();
    assert!(summoner.due(start));
    assert!(!summoner.due(start + Duration::from_millis(300)));
    assert!(summoner.due(start + Duration::from_millis(700)));
}
