//! PIN entry (#207), as the webview's `components/accounts/PinPad`: a
//! field for keys and a number pad for a controller. A pad's A types the
//! focused digit, B deletes (or leaves when there's nothing to delete),
//! Start confirms. Settings → Accounts uses it (#105); sign-in, the age
//! gate and admin approval (#106) share it. The PIN lives only in this
//! component until it goes to the core.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use caskade_core::accounts::PinCheck;
use caskade_ui_i18n::t;
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use crate::input::{Bind, use_input_action};
use crate::ui::icons::{CHECK, DELETE};
use crate::ui::{Button, Svg};

/// A PIN check's answer, later.
pub type Checking = Pin<Box<dyn Future<Output = Result<PinCheck, String>>>>;

/// What a PIN is checked with: the core's answer, or why it was refused.
#[derive(Clone)]
pub struct Check(pub Rc<dyn Fn(String) -> Checking>);

impl Check {
    pub fn new<F, Fut>(check: F) -> Self
    where
        F: Fn(String) -> Fut + 'static,
        Fut: Future<Output = Result<PinCheck, String>> + 'static,
    {
        Self(Rc::new(move |pin| Box::pin(check(pin))))
    }
}

impl PartialEq for Check {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// 4 to 8 digits.
pub fn valid_pin(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.bytes().all(|byte| byte.is_ascii_digit())
}

/// "0:27", "12:05".
fn wait(ms: i64) -> String {
    let seconds = (ms.max(0) + 999) / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

async fn sleep(time: Duration) {
    // Timers need the app runtime's reactor; Dioxus's executor has none.
    let _ = caskade_app::runtime::spawn(async move { tokio::time::sleep(time).await }).await;
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Ready,
    Wrong,
}

const KEY: &str = "grid h-14 place-items-center rounded-control border border-border-strong bg-surface-overlay/70 font-display text-xl font-semibold text-fg outline-none transition-[background-color,border-color,scale] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover data-pressed:scale-[0.96]";

/// A PIN for `title`, checked by `check`; `on_matched` when it matched.
#[component]
pub fn PinPad(
    /// What the PIN is for: "Enter Ada's PIN".
    title: String,
    check: Check,
    on_matched: EventHandler<()>,
    /// B with nothing to delete, or Escape.
    #[props(default)]
    on_cancel: Option<EventHandler<()>>,
    /// When wrong PINs have already locked it (Unix ms).
    #[props(default)]
    locked_until_ms: Option<i64>,
    /// Off where a dialog's heading already says it; it still names the field.
    #[props(default = true)]
    show_title: bool,
) -> Element {
    let mut pin = use_signal(String::new);
    let mut status = use_signal(|| Status::Ready);
    let mut message = use_signal(|| None::<String>);
    let mut until =
        use_signal(move || locked_until_ms.filter(|until| *until > caskade_core::now_ms()));
    let mut pending = use_signal(|| false);
    let mut now = use_signal(caskade_core::now_ms);
    let mut root = use_signal(|| None::<blitz_dom::NodeId>);
    // The time now, again every second while a wait runs.
    use_hook(move || {
        spawn(async move {
            loop {
                sleep(Duration::from_secs(1)).await;
                if until.peek().is_some() {
                    now.set(caskade_core::now_ms());
                    if until
                        .peek()
                        .is_some_and(|until| until <= caskade_core::now_ms())
                    {
                        until.set(None);
                        message.set(None);
                    }
                }
            }
        });
    });
    let locked = until().is_some_and(|until| until > now());
    let mut type_digit = move |digit: char| {
        if locked || pending() {
            return;
        }
        status.set(Status::Ready);
        message.set(None);
        if pin.peek().len() < 8 {
            pin.write().push(digit);
        }
    };
    let mut erase = move || {
        pin.write().pop();
    };
    let submit = {
        let check = check.clone();
        use_callback(move |()| {
            if locked || pending() {
                return;
            }
            let entered = pin();
            if !valid_pin(&entered) {
                message.set(Some(t!("A PIN is 4 to 8 digits.")));
                return;
            }
            pending.set(true);
            let check = check.clone();
            spawn(async move {
                let answer = (check.0)(entered).await;
                pending.set(false);
                pin.set(String::new());
                match answer {
                    Ok(PinCheck::Matched) => on_matched.call(()),
                    Ok(PinCheck::Locked { until_ms }) => {
                        until.set(Some(until_ms));
                        now.set(caskade_core::now_ms());
                        status.set(Status::Ready);
                        message.set(None);
                    }
                    Ok(PinCheck::Wrong) => {
                        status.set(Status::Wrong);
                        message.set(Some(t!("That PIN isn't right.")));
                    }
                    Err(reason) => {
                        status.set(Status::Wrong);
                        message.set(Some(reason));
                    }
                }
            });
        })
    };
    let delete_label = caskade_ui_i18n::tp!(note = "Deletes the last digit", "PIN pad", "Delete");
    let confirm_label = caskade_ui_i18n::tp!("PIN pad", "Confirm");
    use_input_action(
        Action::Confirm,
        Bind {
            label: Some(caskade_ui_i18n::tp!(
                note = "Types the focused digit",
                "PIN pad",
                "Type"
            )),
            owner: root(),
            ..Bind::default()
        },
        || false,
    );
    let has_pin = !pin().is_empty();
    use_input_action(
        Action::Back,
        Bind {
            label: Some(if has_pin || on_cancel.is_none() {
                delete_label.clone()
            } else {
                caskade_ui_i18n::tp!("PIN pad", "Back")
            }),
            owner: root(),
            ..Bind::default()
        },
        move || {
            if !pin.peek().is_empty() {
                erase();
                return true;
            }
            match on_cancel {
                Some(on_cancel) => {
                    on_cancel.call(());
                    true
                }
                None => false,
            }
        },
    );
    use_input_action(
        Action::Menu,
        Bind {
            label: Some(confirm_label.clone()),
            owner: root(),
            ..Bind::default()
        },
        move || {
            submit.call(());
            true
        },
    );
    let shown = if locked {
        "locked"
    } else if status() == Status::Wrong {
        "wrong"
    } else {
        "ready"
    };
    let text = if locked {
        Some(t!(
            "Too many wrong PINs. Try again in {time}.",
            time = wait(until().unwrap_or(0) - now())
        ))
    } else {
        message()
    };
    rsx! {
        div {
            "data-cs": "pin-pad",
            "data-status": shown,
            class: "flex w-full max-w-xs flex-col items-center gap-4",
            onmounted: move |event| {
                if let Some(handle) = event.data().downcast::<NodeHandle>() {
                    root.set(Some(handle.node_id()));
                }
            },
            // Digits typed anywhere on the pad go to the PIN.
            onkeydown: move |event: KeyboardEvent| match event.key() {
                Key::Character(text) if text.len() == 1 && text.chars().all(|c| c.is_ascii_digit()) => {
                    event.prevent_default();
                    event.stop_propagation();
                    type_digit(text.chars().next().unwrap_or('0'));
                }
                Key::Named(NamedKey::Backspace) => {
                    event.prevent_default();
                    erase();
                }
                Key::Named(NamedKey::Enter) => {
                    event.prevent_default();
                    submit.call(());
                }
                Key::Named(NamedKey::Escape) => {
                    if let Some(on_cancel) = on_cancel {
                        event.prevent_default();
                        event.stop_propagation();
                        on_cancel.call(());
                    }
                }
                _ => {}
            },
            if show_title {
                h2 { "data-part": "title", class: "text-center font-display text-lg font-semibold", "{title}" }
            }
            input {
                "data-part": "field",
                r#type: "password",
                inputmode: "numeric",
                autocomplete: "off",
                "aria-label": "{title}",
                "aria-invalid": (shown == "wrong").then_some("true"),
                readonly: locked,
                maxlength: "8",
                value: "{pin}",
                class: if shown == "wrong" {
                    "h-14 w-full rounded-control border border-danger bg-surface-base/60 text-center font-mono text-2xl tracking-[0.5em] text-fg outline-none animate-shake focus-visible:focus-ring"
                } else {
                    "h-14 w-full rounded-control border border-border-strong bg-surface-base/60 text-center font-mono text-2xl tracking-[0.5em] text-fg outline-none focus-visible:focus-ring"
                },
                oninput: move |event| {
                    if locked {
                        return;
                    }
                    status.set(Status::Ready);
                    message.set(None);
                    pin.set(event.value().chars().filter(char::is_ascii_digit).take(8).collect());
                },
                // The field types for itself; the pad's own keys stay out.
                onkeydown: move |event: KeyboardEvent| match event.key() {
                    Key::Named(NamedKey::Enter) => {
                        event.prevent_default();
                        event.stop_propagation();
                        submit.call(());
                    }
                    Key::Named(NamedKey::Escape) => {}
                    _ => event.stop_propagation(),
                },
            }
            p {
                "data-part": "message",
                role: "status",
                hidden: text.is_none(),
                class: if locked || shown == "wrong" { "min-h-5 text-center text-sm text-danger" } else { "min-h-5 text-center text-sm text-fg-muted" },
                {text.clone().unwrap_or_default()}
            }
            div { "data-part": "keys", class: "grid w-full grid-cols-3 gap-2",
                for digit in ['1', '2', '3', '4', '5', '6', '7', '8', '9'] {
                    Button {
                        key: "{digit}",
                        "data-part": "key",
                        "data-focus-default": (digit == '1').then_some("true"),
                        class: KEY,
                        on_press: move |_| type_digit(digit),
                        "{digit}"
                    }
                }
                Button {
                    "data-part": "delete",
                    "aria-label": delete_label,
                    class: KEY,
                    on_press: move |_| erase(),
                    Svg { icon: DELETE, class: "size-5" }
                }
                Button {
                    "data-part": "key",
                    class: KEY,
                    on_press: move |_| type_digit('0'),
                    "0"
                }
                Button {
                    "data-part": "submit",
                    "aria-label": confirm_label,
                    disabled: pending(),
                    class: "{KEY} bg-linear-to-r from-accent to-accent-strong text-accent-fg",
                    on_press: move |_| submit.call(()),
                    Svg { icon: CHECK, class: "size-5" }
                }
            }
        }
    }
}
