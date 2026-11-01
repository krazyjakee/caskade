//! #146: an injected pad press reaches the webview as `inputActionEvent`,
//! only while Caskade has focus (except `guide`), on Tauri's mock runtime.
use super::*;
use caskade_input::{Button, GlyphFamily, Raw, Timing};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{
    Listener,
    test::{mock_builder, mock_context, noop_assets},
};

#[test]
fn injected_pad_input_reaches_the_webview_while_focused() {
    let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
        .commands(tauri_specta::collect_commands![input_devices])
        .events(tauri_specta::collect_events![InputActionEvent]);
    let app = mock_builder()
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    specta.mount_events(&app);
    let bus = Bus::default();
    let mut bus_events = bus.stream();
    let focused = Arc::new(AtomicBool::new(true));
    let summoned = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let service = InputService::without_pads(
        sink(
            app.handle().clone(),
            Some(bus),
            {
                let focused = focused.clone();
                move |_| focused.load(Ordering::SeqCst)
            },
            {
                let summoned = summoned.clone();
                move |app| {
                    summoned.fetch_add(1, Ordering::SeqCst);
                    // The real raise runs too; the mock window accepts it.
                    raise(app);
                }
            },
        ),
        Timing::default(),
    );
    app.manage(service.clone());
    let received = Arc::new(Mutex::new(Vec::<InputAction>::new()));
    let seen = received.clone();
    app.listen(InputActionEvent::NAME, move |event| {
        let action: InputAction = serde_json::from_str(event.payload()).unwrap();
        seen.lock().unwrap().push(action);
    });
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
    let payload = serde_json::to_value(first).unwrap();
    assert_eq!(payload["action"], "confirm");
    assert_eq!(payload["source"], "pad");
    assert!(matches!(
        bus_events.try_recv(),
        Ok(Event::InputDevicesChanged { connected: 1 })
    ));
    let devices = app.state::<InputService>().devices();
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
