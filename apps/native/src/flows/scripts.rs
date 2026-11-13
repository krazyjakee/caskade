//! One scope's lifecycle scripts (#180), as the webview's
//! `components/scripts/ScriptsPanel`: in Settings → Scripts (global), an
//! emulator profile and the game editor (#100). Their order and what each
//! runs, a deliberate Test through the launch's own runner with its
//! output, and the order a launch takes across every scope.

use caskade_core::error::ErrorReport;
use caskade_ui_data::scripts::{
    LifecycleScript, ScriptCommand, ScriptPhase, ScriptScope, ScriptStatus, ScriptStep, ScriptTest,
    Scripts, Shell,
};
use caskade_ui_i18n::{t, tp};
use dioxus_native::prelude::*;

use super::fields::{ListField, PathField};
use super::paths::{PathRequest, PickKind};
use crate::data::change;
use crate::ui::icons::{ARROW_DOWN, ARROW_UP, COPY, FLASK_CONICAL, PLUS, SQUARE, TRASH};
use crate::ui::{
    Button, ButtonVariant, Segment, SegmentedControl, Select, SelectOption, Switch, TextField,
};

/// The app's scripts: a test's, else the running app's.
pub fn use_scripts() -> Option<Scripts> {
    try_use_context::<Scripts>().or_else(|| crate::shell::services().map(Scripts::of))
}

fn phase_label(phase: ScriptPhase) -> String {
    match phase {
        ScriptPhase::BeforeLaunch => tp!("script phase", "Before launch"),
        ScriptPhase::AfterStart => tp!("script phase", "After the game starts"),
        ScriptPhase::AfterExit => tp!("script phase", "After the game exits"),
    }
}

const PHASES: [ScriptPhase; 3] = [
    ScriptPhase::BeforeLaunch,
    ScriptPhase::AfterStart,
    ScriptPhase::AfterExit,
];

fn phase_id(phase: ScriptPhase) -> &'static str {
    match phase {
        ScriptPhase::BeforeLaunch => "before_launch",
        ScriptPhase::AfterStart => "after_start",
        ScriptPhase::AfterExit => "after_exit",
    }
}

fn scope_label(scope: &ScriptScope) -> String {
    match scope {
        ScriptScope::Global => tp!("script scope", "Global"),
        ScriptScope::Profile { .. } => tp!("script scope", "Emulator profile"),
        ScriptScope::Game { .. } => tp!("script scope", "This game"),
    }
}

/// A shell's name, as its program is called (not translated).
fn shell_id(shell: Shell) -> &'static str {
    match shell {
        Shell::Sh => "sh",
        Shell::Bash => "bash",
        Shell::Pwsh => "pwsh",
        Shell::Powershell => "powershell",
        Shell::Cmd => "cmd",
    }
}

fn shell_of(id: &str) -> Option<Shell> {
    [
        Shell::Sh,
        Shell::Bash,
        Shell::Pwsh,
        Shell::Powershell,
        Shell::Cmd,
    ]
    .into_iter()
    .find(|shell| shell_id(*shell) == id)
}

/// Written as they are; never a translatable placeholder.
const PLACEHOLDERS: &str = "{InstallDir}, {GameTitle}";

/// A new script of `scope`, before it's saved (an empty id).
fn blank(scope: ScriptScope, name: String) -> LifecycleScript {
    LifecycleScript {
        id: String::new(),
        scope,
        name,
        enabled: true,
        phase: ScriptPhase::BeforeLaunch,
        command: ScriptCommand::Shell {
            shell: Shell::available()[0],
            script: String::new(),
        },
        cwd: None,
        timeout_secs: 60,
    }
}

/// `scope`'s scripts. `game_id` is the game whose launch order shows, and
/// whose values a test uses; `on_dirty_change` hears an unsaved edit start
/// and end. Nothing where the app has no scripts.
#[component]
pub fn ScriptsPanel(
    scope: ScriptScope,
    #[props(default)] game_id: Option<String>,
    #[props(default)] profile_id: Option<String>,
    #[props(default)] on_dirty_change: Option<EventHandler<bool>>,
) -> Element {
    let Some(scripts) = use_scripts() else {
        return rsx! {};
    };
    rsx! {
        ScriptsOf { scripts, scope, game_id, profile_id, on_dirty_change }
    }
}

#[component]
fn ScriptsOf(
    scripts: Scripts,
    scope: ScriptScope,
    game_id: Option<String>,
    profile_id: Option<String>,
    on_dirty_change: Option<EventHandler<bool>>,
) -> Element {
    // Changes read everything again.
    let mut version = use_signal(|| 0u32);
    let list = {
        let (scripts, scope) = (scripts.clone(), scope.clone());
        use_resource(move || {
            let (scripts, scope) = (scripts.clone(), scope.clone());
            let _ = version();
            async move { change(async move { scripts.list(&scope).await }).await }
        })
    };
    let mut selected = use_signal(|| None::<String>);
    let mut draft = use_signal(|| None::<LifecycleScript>);
    let mut confirm_delete = use_signal(|| false);
    let mut saving = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let list_read = list.read().clone();
    let all = match list_read {
        None => {
            return rsx! {
                p { role: "status", class: "text-sm text-fg-muted", {t!("Loading scripts…")} }
            };
        }
        Some(Err(report)) => {
            return rsx! {
                p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
            };
        }
        Some(Ok(all)) => all,
    };
    let chosen = selected().or_else(|| all.first().map(|script| script.id.clone()));
    let shown = all
        .iter()
        .find(|script| Some(&script.id) == chosen.as_ref())
        .cloned();
    let editing = draft().or_else(|| shown.clone());
    let saved = match draft() {
        Some(draft) if Some(&draft.id) != shown.as_ref().map(|shown| &shown.id) => None,
        _ => shown.clone(),
    };
    let dirty = draft().is_some_and(|draft| Some(&draft) != saved.as_ref());
    let mut was_dirty = use_signal(|| false);
    if *was_dirty.peek() != dirty {
        was_dirty.set(dirty);
        if let Some(on_dirty_change) = on_dirty_change {
            on_dirty_change.call(dirty);
        }
    }
    let mut choose = move |id: Option<String>| {
        selected.set(id);
        draft.set(None);
        confirm_delete.set(false);
        failure.set(None);
    };
    let start_new = {
        let scope = scope.clone();
        move |_| {
            selected.set(None);
            failure.set(None);
            draft.set(Some(blank(scope.clone(), t!("New script"))));
        }
    };
    let save = {
        let (scripts, scope) = (scripts.clone(), scope.clone());
        move |script: LifecycleScript| {
            let (scripts, scope) = (scripts.clone(), scope.clone());
            saving.set(true);
            failure.set(None);
            spawn(async move {
                let result = change(async move { scripts.save(&scope, script).await }).await;
                saving.set(false);
                match result {
                    Ok(script) => {
                        selected.set(Some(script.id));
                        draft.set(None);
                        version += 1;
                    }
                    Err(report) => failure.set(Some(report)),
                }
            });
        }
    };
    let move_by = {
        let (scripts, scope, all) = (scripts.clone(), scope.clone(), all.clone());
        move |id: String, by: isize| {
            let mut ids: Vec<String> = all.iter().map(|script| script.id.clone()).collect();
            let Some(at) = ids.iter().position(|each| *each == id) else {
                return;
            };
            let to = at as isize + by;
            if to < 0 || to as usize >= ids.len() {
                return;
            }
            let moved = ids.remove(at);
            ids.insert(to as usize, moved);
            let (scripts, scope) = (scripts.clone(), scope.clone());
            busy.set(true);
            failure.set(None);
            spawn(async move {
                let result = change(async move { scripts.order(&scope, &ids).await }).await;
                busy.set(false);
                match result {
                    Ok(()) => version += 1,
                    Err(report) => failure.set(Some(report)),
                }
            });
        }
    };
    let delete = {
        let (scripts, scope) = (scripts.clone(), scope.clone());
        move |id: String| {
            if !confirm_delete() {
                confirm_delete.set(true);
                return;
            }
            let (scripts, scope) = (scripts.clone(), scope.clone());
            busy.set(true);
            failure.set(None);
            spawn(async move {
                let result = change(async move { scripts.delete(&scope, &id).await }).await;
                busy.set(false);
                match result {
                    Ok(_) => {
                        choose(None);
                        version += 1;
                    }
                    Err(report) => failure.set(Some(report)),
                }
            });
        }
    };
    let first = all.first().map(|script| script.id.clone());
    let last = all.last().map(|script| script.id.clone());
    rsx! {
        div { "data-cs": "scripts", class: "flex flex-col gap-5",
            div { class: "grid gap-5 md:grid-cols-[minmax(12rem,16rem)_1fr]",
                div { class: "flex flex-col gap-3",
                    if all.is_empty() {
                        p { class: "text-sm text-fg-muted", {t!("No scripts here yet.")} }
                    } else {
                        div {
                            "data-part": "items",
                            role: "listbox",
                            "aria-label": t!("Scripts"),
                            class: "flex flex-col gap-1",
                            for script in all.clone() {
                                {
                                    let current = editing.as_ref().is_some_and(|editing| editing.id == script.id);
                                    let id = script.id.clone();
                                    rsx! {
                                        Button {
                                            key: "{script.id}",
                                            role: "option",
                                            "aria-selected": if current { "true" } else { "false" },
                                            "data-selected": current.then_some("true"),
                                            "data-part": "script",
                                            "data-enabled": if script.enabled { "true" } else { "false" },
                                            "data-spatial": "focusable",
                                            variant: ButtonVariant::Ghost,
                                            disabled: dirty && !current,
                                            class: if current { "flex-col items-start border border-accent-alt bg-accent/10 text-left" } else { "flex-col items-start border border-border text-left" },
                                            on_press: move |_| {
                                                if !dirty {
                                                    choose(Some(id.clone()));
                                                }
                                            },
                                            span { class: "truncate font-medium", "{script.name}" }
                                            span { class: "text-xs text-fg-muted",
                                                {phase_label(script.phase)}
                                                if !script.enabled {
                                                    " · "
                                                    {tp!("a disabled script", "Off")}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Button {
                        id: "scripts-add",
                        "data-spatial": "focusable",
                        icon: PLUS,
                        disabled: dirty,
                        on_press: start_new,
                        {t!("Add script")}
                    }
                }
                if let Some(script) = editing.clone() {
                    div { class: "flex flex-col gap-4",
                        ScriptForm {
                            script: script.clone(),
                            disabled: saving(),
                            on_change: move |next| draft.set(Some(next)),
                        }
                        if let Some(report) = failure() {
                            p { role: "alert", "data-part": "error", class: "text-sm text-danger", "{report.summary}" }
                        }
                        div { class: "flex flex-wrap items-center gap-2",
                            Button {
                                id: "script-save",
                                "data-spatial": "focusable",
                                variant: ButtonVariant::Primary,
                                disabled: !dirty || saving(),
                                on_press: {
                                    let (mut save, script) = (save.clone(), script.clone());
                                    move |_| save(script.clone())
                                },
                                if saving() { {t!("Saving…")} } else { {t!("Save script")} }
                            }
                            Button {
                                id: "script-revert",
                                "data-spatial": "focusable",
                                disabled: !dirty || saving(),
                                on_press: move |_| {
                                    draft.set(None);
                                    failure.set(None);
                                },
                                {t!("Revert")}
                            }
                            if let Some(saved) = saved.clone() {
                                Button {
                                    id: "script-duplicate",
                                    "data-spatial": "focusable",
                                    icon: COPY,
                                    disabled: dirty,
                                    on_press: {
                                        let saved = saved.clone();
                                        move |_| {
                                            selected.set(None);
                                            failure.set(None);
                                            // A copy starts off: copying never turns on a
                                            // second command unseen.
                                            draft.set(Some(LifecycleScript {
                                                id: String::new(),
                                                enabled: false,
                                                name: t!(note = "A duplicated script's name", "{0} (copy)", 0 = saved.name.clone()),
                                                ..saved.clone()
                                            }));
                                        }
                                    },
                                    {t!("Duplicate")}
                                }
                                Button {
                                    id: "script-up",
                                    "data-spatial": "focusable",
                                    "aria-label": t!("Move up"),
                                    icon: ARROW_UP,
                                    disabled: dirty || first.as_ref() == Some(&saved.id) || busy(),
                                    on_press: {
                                        let (mut move_by, id) = (move_by.clone(), saved.id.clone());
                                        move |_| move_by(id.clone(), -1)
                                    },
                                }
                                Button {
                                    id: "script-down",
                                    "data-spatial": "focusable",
                                    "aria-label": t!("Move down"),
                                    icon: ARROW_DOWN,
                                    disabled: dirty || last.as_ref() == Some(&saved.id) || busy(),
                                    on_press: {
                                        let (mut move_by, id) = (move_by.clone(), saved.id.clone());
                                        move |_| move_by(id.clone(), 1)
                                    },
                                }
                                Button {
                                    id: "script-delete",
                                    "data-spatial": "focusable",
                                    "data-part": "delete",
                                    variant: ButtonVariant::Danger,
                                    icon: TRASH,
                                    disabled: busy(),
                                    on_press: {
                                        let (mut delete, id) = (delete.clone(), saved.id.clone());
                                        move |_| delete(id.clone())
                                    },
                                    if confirm_delete() {
                                        {t!("Delete “{0}”?", 0 = saved.name.clone())}
                                    } else {
                                        {t!("Delete…")}
                                    }
                                }
                            }
                        }
                        ScriptTester {
                            key: "{script.id}",
                            scripts: scripts.clone(),
                            scope: scope.clone(),
                            script: script.clone(),
                            game_id: game_id.clone(),
                        }
                    }
                }
            }
            EffectiveOrder { scripts, scope, game_id, profile_id, version: version() }
        }
    }
}

#[component]
fn ScriptForm(
    script: LifecycleScript,
    disabled: bool,
    on_change: EventHandler<LifecycleScript>,
) -> Element {
    let set = {
        let script = script.clone();
        move |change_it: Box<dyn FnOnce(&mut LifecycleScript)>| {
            let mut next = script.clone();
            change_it(&mut next);
            on_change.call(next);
        }
    };
    let phases: Vec<SelectOption> = PHASES
        .iter()
        .map(|phase| SelectOption::new(phase_id(*phase), phase_label(*phase)))
        .collect();
    let hint = match script.phase {
        ScriptPhase::BeforeLaunch => t!(
            "If it fails or runs out of time, the game doesn't start and the after-exit scripts clean up."
        ),
        ScriptPhase::AfterStart => {
            t!("Runs beside the game once it is found. A failure is noted; the game carries on.")
        }
        ScriptPhase::AfterExit => {
            t!("Runs once the game has ended, or a launch didn't start. A failure is noted.")
        }
    };
    let kind = match &script.command {
        ScriptCommand::Shell { .. } => "shell",
        ScriptCommand::Direct { .. } => "direct",
    };
    let kinds = vec![
        Segment::new("shell", tp!("script kind", "Shell script")),
        Segment::new("direct", tp!("script kind", "Program")),
    ];
    let cwd = script
        .cwd
        .as_ref()
        .map(|cwd| cwd.to_string_lossy().into_owned())
        .unwrap_or_default();
    rsx! {
        div { "data-part": "form", class: "flex flex-col gap-4",
            TextField {
                id: "script-name",
                label: tp!("script", "Name"),
                value: script.name.clone(),
                disabled,
                on_input: {
                    let set = set.clone();
                    move |name: String| set(Box::new(move |script| script.name = name))
                },
            }
            Switch {
                id: "script-enabled",
                checked: script.enabled,
                disabled,
                label: t!("Runs with launches"),
                on_change: {
                    let set = set.clone();
                    move |enabled: bool| set(Box::new(move |script| script.enabled = enabled))
                },
            }
            Select {
                id: "script-phase",
                label: t!("When"),
                options: phases,
                value: Some(phase_id(script.phase).to_owned()),
                disabled,
                on_change: {
                    let set = set.clone();
                    move |id: String| {
                        if let Some(phase) = PHASES.into_iter().find(|phase| phase_id(*phase) == id) {
                            set(Box::new(move |script| script.phase = phase));
                        }
                    }
                },
            }
            p { class: "text-xs text-fg-muted", "{hint}" }
            SegmentedControl {
                "aria-label": t!("What it runs"),
                options: kinds,
                value: kind.to_owned(),
                disabled,
                on_change: {
                    let set = set.clone();
                    move |kind: String| {
                        set(Box::new(move |script| {
                            script.command = if kind == "shell" {
                                ScriptCommand::Shell { shell: Shell::available()[0], script: String::new() }
                            } else {
                                ScriptCommand::Direct { program: Default::default(), args: Vec::new() }
                            };
                        }))
                    }
                },
            }
            match script.command.clone() {
                ScriptCommand::Shell { shell, script: text } => {
                    let mut options: Vec<Shell> = Shell::available().to_vec();
                    if !options.contains(&shell) {
                        options.insert(0, shell);
                    }
                    let options: Vec<SelectOption> = options
                        .into_iter()
                        .map(|shell| SelectOption::new(shell_id(shell), shell_id(shell)))
                        .collect();
                    rsx! {
                        Select {
                            id: "script-shell",
                            label: t!("Shell"),
                            options,
                            value: Some(shell_id(shell).to_owned()),
                            disabled,
                            on_change: {
                                let (set, text) = (set.clone(), text.clone());
                                move |id: String| {
                                    if let Some(shell) = shell_of(&id) {
                                        let text = text.clone();
                                        set(Box::new(move |script| script.command = ScriptCommand::Shell { shell, script: text }));
                                    }
                                }
                            },
                        }
                        ScriptText {
                            label: t!("Script"),
                            value: text.clone(),
                            disabled,
                            on_change: {
                                let set = set.clone();
                                move |text: String| set(Box::new(move |script| script.command = ScriptCommand::Shell { shell, script: text }))
                            },
                        }
                        p { class: "text-xs text-fg-muted",
                            {t!("The script reads the launch's values from CASKADE_ variables, like \"$CASKADE_INSTALL_DIR\". Nothing is pasted into its text.")}
                        }
                    }
                }
                ScriptCommand::Direct { program, args } => rsx! {
                    PathField {
                        id: "script-program",
                        label: t!("Program"),
                        value: program.to_string_lossy().into_owned(),
                        request: PathRequest::new(PickKind::File, t!("Choose the program")),
                        disabled,
                        on_change: {
                            let (set, args) = (set.clone(), args.clone());
                            move |path: String| {
                                let args = args.clone();
                                set(Box::new(move |script| script.command = ScriptCommand::Direct { program: path.into(), args }))
                            }
                        },
                    }
                    ListField {
                        label: tp!("a program's command-line argument", "Argument"),
                        values: args.clone(),
                        add_label: t!("Add argument"),
                        disabled,
                        on_change: {
                            let (set, program) = (set.clone(), program.clone());
                            move |args: Vec<String>| {
                                let program = program.clone();
                                set(Box::new(move |script| script.command = ScriptCommand::Direct { program, args }))
                            }
                        },
                    }
                    p { class: "text-xs text-fg-muted",
                        {t!("Each argument goes to the program as it is, with no shell: {PLACEHOLDERS} and the other values below are put in as text.", PLACEHOLDERS = PLACEHOLDERS)}
                    }
                },
            }
            PathField {
                id: "script-cwd",
                label: t!("Working folder"),
                description: t!("Empty: the game's install folder."),
                value: cwd,
                request: PathRequest::new(PickKind::Folder, t!("Choose the working folder")),
                disabled,
                on_change: {
                    let set = set.clone();
                    move |cwd: String| set(Box::new(move |script| script.cwd = (!cwd.is_empty()).then(|| cwd.into())))
                },
            }
            TextField {
                id: "script-timeout",
                label: t!("Time limit (seconds)"),
                value: script.timeout_secs.to_string(),
                disabled,
                on_input: {
                    let set = set.clone();
                    move |value: String| {
                        let secs = value.trim().parse().unwrap_or(0);
                        set(Box::new(move |script| script.timeout_secs = secs))
                    }
                },
            }
        }
    }
}

/// The script's text: a multi-line field with its label.
#[component]
fn ScriptText(
    label: String,
    value: String,
    disabled: bool,
    on_change: EventHandler<String>,
) -> Element {
    rsx! {
        label { class: "flex flex-col gap-1.5",
            span { class: "text-sm font-medium", "{label}" }
            div {
                "data-cs": "text-field",
                "data-part": "control",
                class: "flex rounded-control border border-border-strong bg-surface-base/60 focus-within:border-accent-alt/60",
                textarea {
                    id: "script-text",
                    "aria-label": "{label}",
                    "data-part": "input",
                    "data-spatial": "focusable",
                    rows: "6",
                    disabled,
                    value: "{value}",
                    class: "block w-full resize-y bg-transparent px-3 py-2 font-mono text-sm text-fg outline-none",
                    oninput: move |event| on_change.call(event.value()),
                }
            }
        }
    }
}

/// Test: runs the script once now, as a launch would, without a game.
#[component]
fn ScriptTester(
    scripts: Scripts,
    scope: ScriptScope,
    script: LifecycleScript,
    game_id: Option<String>,
) -> Element {
    // A name for this panel's tests, unique in the app.
    let id = use_hook(|| {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        format!("script-test-{}", NEXT.fetch_add(1, Ordering::Relaxed))
    });
    let mut count = use_signal(|| 0u32);
    let mut running = use_signal(|| false);
    let mut result = use_signal(|| None::<ScriptTest>);
    let mut failure = use_signal(|| None::<ErrorReport>);
    let test_id = format!("{id}:{}", count());
    let run = {
        let (scripts, test_id) = (scripts.clone(), test_id.clone());
        move |_| {
            let (scripts, scope, script, game_id, test_id) = (
                scripts.clone(),
                scope.clone(),
                script.clone(),
                game_id.clone(),
                test_id.clone(),
            );
            running.set(true);
            failure.set(None);
            spawn(async move {
                let outcome =
                    change(async move { scripts.test(&scope, &script, game_id, &test_id).await })
                        .await;
                running.set(false);
                count += 1;
                match outcome {
                    Ok(test) => result.set(Some(test)),
                    Err(report) => failure.set(Some(report)),
                }
            });
        }
    };
    let cancel = {
        let (scripts, test_id) = (scripts.clone(), test_id.clone());
        move |_| {
            let (scripts, test_id) = (scripts.clone(), test_id.clone());
            spawn(async move {
                let _ = change(async move { scripts.cancel_test(&test_id).await }).await;
            });
        }
    };
    rsx! {
        div { "data-part": "test", class: "flex flex-col gap-3 border-t border-border pt-3",
            div { class: "flex flex-wrap items-center gap-2",
                Button {
                    id: "script-test",
                    "data-spatial": "focusable",
                    icon: FLASK_CONICAL,
                    disabled: running(),
                    on_press: run,
                    if running() { {t!("Testing…")} } else { {t!("Test")} }
                }
                if running() {
                    Button {
                        id: "script-test-cancel",
                        "data-spatial": "focusable",
                        icon: SQUARE,
                        on_press: cancel,
                        {tp!("stops a running script test", "Cancel test")}
                    }
                }
                span { class: "text-xs text-fg-muted",
                    {t!("Runs it once now, as a launch would, without a game starting.")}
                }
            }
            if let Some(report) = failure() {
                p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
            }
            if let Some(test) = result() {
                TestResult { result: test }
            }
        }
    }
}

fn status_name(status: ScriptStatus) -> &'static str {
    match status {
        ScriptStatus::Succeeded => "succeeded",
        ScriptStatus::Failed => "failed",
        ScriptStatus::TimedOut => "timed_out",
        ScriptStatus::Cancelled => "cancelled",
        ScriptStatus::NotStarted => "not_started",
    }
}

#[component]
fn TestResult(result: ScriptTest) -> Element {
    let run = result.run.clone();
    let status = match run.status {
        ScriptStatus::Succeeded => tp!("script test result", "Finished"),
        ScriptStatus::Failed => match run.exit_code {
            None => t!("Stopped by a signal"),
            Some(code) => tp!("script test result", "Failed with exit code {0}", 0 = code),
        },
        ScriptStatus::TimedOut => t!("Ran out of time"),
        ScriptStatus::Cancelled => tp!("script test result", "Cancelled"),
        ScriptStatus::NotStarted => t!("Didn't start"),
    };
    let duration = t!("{0} ms", 0 = run.duration_ms);
    rsx! {
        div { "data-part": "result", "data-status": status_name(run.status), class: "flex flex-col gap-2 text-sm",
            p { role: "status", class: "font-medium", "{status} · {duration}" }
            if let Some(error) = run.error.clone() {
                p { class: "text-danger", "{error}" }
            }
            if run.truncated {
                p { class: "text-xs text-fg-muted", {t!("The output was long; its end is shown.")} }
            }
            if !run.stdout.is_empty() {
                div {
                    p { class: "text-xs font-semibold", {t!("Output")} }
                    pre { "data-part": "output", class: "max-h-48 overflow-auto rounded-control border border-border bg-surface-base/60 p-2 font-mono text-xs whitespace-pre-wrap",
                        "{run.stdout}"
                    }
                }
            }
            if !run.stderr.is_empty() {
                div {
                    p { class: "text-xs font-semibold", {t!("Errors")} }
                    pre { "data-part": "output", class: "max-h-48 overflow-auto rounded-control border border-border bg-surface-base/60 p-2 font-mono text-xs whitespace-pre-wrap",
                        "{run.stderr}"
                    }
                }
            }
            div { "data-part": "context",
                p { class: "text-xs font-semibold", {t!("Values it saw")} }
                dl { class: "mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 font-mono text-xs",
                    for value in result.context.clone() {
                        dt { key: "{value.variable}", class: "text-fg-muted", "{value.placeholder} · {value.variable}" }
                        dd { class: "break-all", if value.value.is_empty() { "—" } else { "{value.value}" } }
                    }
                }
            }
        }
    }
}

/// The order a launch runs scripts in, every scope's, with the ones from
/// other scopes and the disabled ones marked.
#[component]
fn EffectiveOrder(
    scripts: Scripts,
    scope: ScriptScope,
    game_id: Option<String>,
    profile_id: Option<String>,
    /// Read again when it changes.
    version: u32,
) -> Element {
    let steps = use_resource(use_reactive!(|(version, game_id, profile_id)| {
        let scripts = scripts.clone();
        let _ = version;
        async move {
            change(async move {
                scripts
                    .effective(game_id.as_deref(), profile_id.as_deref())
                    .await
            })
            .await
            .unwrap_or_default()
        }
    }));
    let steps: Vec<ScriptStep> = steps.read().clone().unwrap_or_default();
    rsx! {
        section { "data-part": "order", "aria-label": t!("Order at launch"), class: "flex flex-col gap-2",
            h3 { class: "text-sm font-semibold", {t!("Order at launch")} }
            if steps.is_empty() {
                p { class: "text-xs text-fg-muted", {t!("No scripts run at launch.")} }
            } else {
                for phase in PHASES {
                    {
                        let in_phase: Vec<ScriptStep> = steps.iter().filter(|step| step.phase == phase).cloned().collect();
                        if in_phase.is_empty() {
                            rsx! {}
                        } else {
                            rsx! {
                                div { key: "{phase_id(phase)}",
                                    p { class: "text-xs font-semibold text-fg-muted", {phase_label(phase)} }
                                    ol { class: "ml-4 list-decimal text-sm",
                                        for step in in_phase {
                                            {
                                                let inherited = step.script.scope != scope;
                                                let label = if inherited {
                                                    t!("{0}, inherited", 0 = scope_label(&step.script.scope))
                                                } else {
                                                    scope_label(&step.script.scope)
                                                };
                                                rsx! {
                                                    li {
                                                        key: "{step.script.id}",
                                                        "data-part": "step",
                                                        "data-inherited": inherited.then_some("true"),
                                                        "data-enabled": if step.script.enabled { "true" } else { "false" },
                                                        class: if step.script.enabled { "" } else { "text-fg-subtle line-through" },
                                                        "{step.script.name}"
                                                        span { class: "ml-2 text-xs text-fg-muted no-underline",
                                                            "{label}"
                                                            if !step.script.enabled {
                                                                " · "
                                                                {tp!("a disabled script", "Off")}
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
