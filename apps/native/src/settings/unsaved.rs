//! Explicit-save forms in Settings (#43; the webview's `unsavedChanges.tsx`).
//! Most settings save as they change; a form with a Save button holds its
//! edits until it's pressed, and a section's panel is replaced when another
//! opens. Such a form registers here while it has edits
//! ([`use_unsaved_changes`]), and leaving its section asks first: Save,
//! Discard or Stay. Leaving is another section or sub-section (a click, the
//! keyboard, LB/RB), another page, Back, an account or mode switch
//! ([`after_unsaved_changes`]), or a close of the window that would quit
//! ([`holds_close`]).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use caskade_ui_i18n::t;
use dioxus_native::prelude::*;

use crate::router::{Guard, Route, use_router};
use crate::ui::{Button, ButtonVariant, Dialog};

/// A form with edits, as the question names and settles it.
#[derive(Clone)]
pub struct UnsavedForm {
    /// What holds the edits, named in the question ("My profile").
    pub label: String,
    /// Saves the edits; whether they saved.
    pub save: Rc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool>>>>,
    /// Drops the edits.
    pub discard: Rc<dyn Fn()>,
}

/// The forms with edits, by registration.
#[derive(Clone, Copy, PartialEq)]
struct Forms(Signal<BTreeMap<usize, CopyValue<UnsavedForm>>>);

/// What the edits would be lost to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code, reason = "account and mode switches (#104) leave with them")]
pub enum Leaving {
    Section,
    Account,
    Mode,
    Window,
}

/// A question open about the edits: `leave` follows Save or Discard.
#[derive(Clone)]
struct Question {
    leaving: Leaving,
    leave: Rc<dyn Fn()>,
}

impl PartialEq for Question {
    fn eq(&self, other: &Self) -> bool {
        self.leaving == other.leaving && Rc::ptr_eq(&self.leave, &other.leave)
    }
}

/// Asks the open guard's question about leaving, then runs the action
/// unless the answer is Stay.
type Ask = Rc<dyn Fn(Leaving, Rc<dyn Fn()>)>;

thread_local! {
    /// Asks the open guard's question, then runs the action unless the
    /// answer is Stay: set while a guard is up.
    static ASK: RefCell<Option<Ask>> = const { RefCell::new(None) };
    /// Whether a form has edits now, for the window's close.
    static DIRTY: RefCell<bool> = const { RefCell::new(false) };
    /// A close the question let through.
    static CLOSING: RefCell<bool> = const { RefCell::new(false) };
}

/// For what replaces Settings without a navigation, from outside the
/// guard: runs `proceed` straight away when no form has edits or Settings
/// isn't open, otherwise once the question is answered with Save or
/// Discard.
#[allow(dead_code, reason = "account and mode switches (#104) call it")]
pub fn after_unsaved_changes(leaving: Leaving, proceed: impl Fn() + 'static) {
    let ask = ASK.with(|ask| ask.borrow().clone());
    match ask {
        Some(ask) => ask(leaving, Rc::new(proceed)),
        None => proceed(),
    }
}

/// Whether a close of the window that would quit must wait for the
/// question; asks it if so. A close it let through goes.
pub fn holds_close() -> bool {
    if CLOSING.with(|closing| closing.replace(false)) {
        return false;
    }
    if !DIRTY.with(|dirty| *dirty.borrow()) {
        return false;
    }
    let ask = ASK.with(|ask| ask.borrow().clone());
    let Some(ask) = ask else {
        return false;
    };
    if let Some(window) = crate::shell::main_window() {
        window.set_visible(true);
        window.set_minimized(false);
        window.focus_window();
    }
    ask(
        Leaving::Window,
        Rc::new(|| {
            CLOSING.with(|closing| *closing.borrow_mut() = true);
            crate::launch::ask(crate::launch::Request::Close);
        }),
    );
    true
}

/// Registers a form while `dirty`, so leaving its section asks first.
/// Outside Settings (no guard above it) it does nothing.
#[allow(dead_code, reason = "the explicit-save forms of #104 and #105 call it")]
pub fn use_unsaved_changes(dirty: bool, form: UnsavedForm) {
    let forms = try_use_context::<Forms>();
    let id = use_hook(|| {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    });
    // The latest save and discard, each render.
    let mut latest = use_hook(|| CopyValue::new(form.clone()));
    latest.set(form);
    use_effect(use_reactive!(|dirty| {
        if let Some(Forms(mut forms)) = forms {
            if dirty {
                forms.write().insert(id, latest);
            } else if forms.peek().contains_key(&id) {
                forms.write().remove(&id);
            }
        }
    }));
    use_drop(move || {
        if let Some(Forms(mut forms)) = forms {
            forms.write().remove(&id);
        }
    });
}

/// The section a page shows: Settings' tab and sub-section, or the page.
fn section_of(route: &Route) -> String {
    match route {
        Route::Settings { tab, section } => {
            format!("settings\n{tab}\n{}", section.as_deref().unwrap_or(""))
        }
        other => other.href(),
    }
}

/// Around Settings' sections: asks before a navigation replaces a panel
/// whose forms have edits, and before an account switch or a close of the
/// window drops them.
#[component]
pub fn UnsavedChangesGuard(children: Element) -> Element {
    let forms = use_context_provider(|| Forms(Signal::new(BTreeMap::new())));
    let router = use_router();
    let mut question = use_signal(|| None::<Question>);
    let dirty = !forms.0.read().is_empty();
    DIRTY.with(|flag| *flag.borrow_mut() = dirty);
    // Navigations away from the section wait while a form has edits.
    use_effect(use_reactive!(|dirty| {
        router.set_guard(dirty.then(|| {
            Guard(Rc::new(|current: &Route, next: &Route| {
                section_of(current) != section_of(next)
            }))
        }));
    }));
    use_hook(move || {
        let ask: Ask = Rc::new(move |leaving, leave| {
            if forms.0.peek().is_empty() {
                leave();
            } else {
                let mut question = question;
                question.set(Some(Question { leaving, leave }));
            }
        });
        ASK.with(|slot| *slot.borrow_mut() = Some(ask));
    });
    use_drop(move || {
        ASK.with(|slot| *slot.borrow_mut() = None);
        DIRTY.with(|flag| *flag.borrow_mut() = false);
        router.set_guard(None);
        router.stay();
    });
    let held = router.held();
    let open = question().or_else(|| {
        held.map(|_| Question {
            leaving: Leaving::Section,
            leave: Rc::new(move || router.proceed()),
        })
    });
    let held_forms: Vec<UnsavedForm> = forms
        .0
        .read()
        .values()
        .map(|form| form.read().clone())
        .collect();
    rsx! {
        {children}
        if let Some(open) = open {
            UnsavedChangesDialog {
                key: "{open.leaving:?}",
                leaving: open.leaving,
                forms: held_forms.iter().map(|form| form.label.clone()).collect::<Vec<_>>(),
                on_save: {
                    let held_forms = held_forms.clone();
                    let leave = open.leave.clone();
                    move |done: Callback<bool, ()>| {
                        let held_forms = held_forms.clone();
                        let leave = leave.clone();
                        spawn(async move {
                            let mut saved = true;
                            for form in &held_forms {
                                if !(form.save)().await {
                                    saved = false;
                                }
                            }
                            done(saved);
                            if saved {
                                question.set(None);
                                leave();
                            }
                        });
                    }
                },
                on_discard: {
                    let held_forms = held_forms.clone();
                    let leave = open.leave.clone();
                    move |_| {
                        for form in &held_forms {
                            (form.discard)();
                        }
                        question.set(None);
                        leave();
                    }
                },
                on_stay: move |_| {
                    question.set(None);
                    router.stay();
                },
            }
        }
    }
}

#[component]
fn UnsavedChangesDialog(
    leaving: Leaving,
    forms: Vec<String>,
    /// Saves every form, then calls back whether all saved.
    on_save: EventHandler<Callback<bool, ()>>,
    on_discard: EventHandler<()>,
    on_stay: EventHandler<()>,
) -> Element {
    let mut saving = use_signal(|| false);
    let mut failed = use_signal(|| false);
    let explanation = match leaving {
        Leaving::Window => {
            t!("These changes aren't saved yet. Closing Caskade without saving loses them.")
        }
        Leaving::Account => {
            t!("These changes aren't saved yet. Switching accounts without saving loses them.")
        }
        Leaving::Mode => {
            t!("These changes aren't saved yet. Switching modes without saving loses them.")
        }
        Leaving::Section => {
            t!("These changes aren't saved yet. Leaving this section without saving loses them.")
        }
    };
    let pending = saving();
    rsx! {
        Dialog {
            title: t!("Save your changes?"),
            alert: true,
            on_close: move |_| {
                if !*saving.peek() {
                    on_stay.call(());
                }
            },
            div { class: "flex flex-col gap-4",
                p { class: "text-sm text-fg-muted", "{explanation}" }
                ul { class: "list-inside list-disc text-sm",
                    for form in forms {
                        li { key: "{form}", "{form}" }
                    }
                }
                if failed() {
                    p { role: "alert", class: "text-sm text-danger",
                        {t!("Some changes couldn't be saved. Stay to fix them, or discard them and leave.")}
                    }
                }
                div { class: "flex flex-wrap justify-end gap-2",
                    Button {
                        variant: ButtonVariant::Danger,
                        disabled: pending,
                        on_press: move |_| on_discard.call(()),
                        {t!(note = "Leave a settings section without saving its changes", "Discard")}
                    }
                    Button {
                        disabled: pending,
                        on_press: move |_| on_stay.call(()),
                        "data-focus-default": "true",
                        {t!(note = "Keep editing the unsaved settings", "Stay")}
                    }
                    Button {
                        variant: ButtonVariant::Primary,
                        disabled: pending,
                        on_press: move |_| {
                            saving.set(true);
                            failed.set(false);
                            on_save.call(Callback::new(move |saved: bool| {
                                saving.set(false);
                                failed.set(!saved);
                            }));
                        },
                        if pending { {t!("Saving…")} } else { {t!("Save")} }
                    }
                }
            }
        }
    }
}
