//! The work the choices start (#97), as the webview's
//! `onboarding/SetupStep.tsx`: each task's progress, the library's count
//! climbing and covers arriving behind it. Then the way in.

use crate::data::sleep;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use caskade_core::model::{GameFilter, GameSort, ListGames};
use caskade_ui_data::Key as DataKey;
use caskade_ui_data::emulation::Emulation;

use caskade_ui_i18n::{format, t, tn};
use dioxus_native::prelude::*;

use super::run::{Choices, Report, SetupParts, TaskId, TaskState, planned_tasks, run_setup};
use super::sound::{Cue, Sounds};
use crate::data::{use_library_stats, use_query};
use crate::ui::icons::{CIRCLE_ALERT, CIRCLE_CHECK, CIRCLE_DASHED, CIRCLE_X, LOADER_CIRCLE};
use crate::ui::{Button, GameArt, Icon, Svg};
use dioxus_native::prelude::dioxus_core::{Task, spawn_forever};

/// How often the elapsed and quiet times move on.
const TICK: Duration = Duration::from_secs(1);
/// How often the count and covers are read again while the work runs.
const REFRESH: Duration = Duration::from_millis(1500);
/// How long the count takes to climb to a new total.
const COUNT_UP: Duration = Duration::from_millis(900);
/// Between the count's steps while it climbs.
const COUNT_FRAME: Duration = Duration::from_millis(33);
/// Seconds without news before setup says so.
const QUIET: u64 = 30;

const OPEN: &str = "inline-flex h-12 items-center justify-center rounded-control bg-linear-to-r from-accent to-accent-strong px-10 font-display text-base font-semibold tracking-widest text-accent-fg uppercase transition-[filter,box-shadow,scale] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:shadow-[0_0_28px_-4px] data-hovered:shadow-glow data-hovered:brightness-115 data-pressed:scale-[0.97]";

fn icon(state: TaskState) -> Icon {
    match state {
        TaskState::Waiting | TaskState::Background | TaskState::Skipped => CIRCLE_DASHED,
        TaskState::Running => LOADER_CIRCLE,
        TaskState::Done => CIRCLE_CHECK,
        TaskState::Warning => CIRCLE_ALERT,
        TaskState::Failed => CIRCLE_X,
    }
}

fn tone(state: TaskState) -> &'static str {
    match state {
        TaskState::Waiting | TaskState::Skipped => "text-fg-subtle",
        TaskState::Background => "text-fg-muted",
        TaskState::Running => "text-accent-alt animate-worker motion-reduce:animate-none",
        TaskState::Done => "text-status-installed",
        TaskState::Warning => "text-warning",
        TaskState::Failed => "text-danger",
    }
}

fn task_name(id: TaskId) -> &'static str {
    match id {
        TaskId::Games => "games",
        TaskId::Emulators => "emulators",
        TaskId::Roms => "roms",
        TaskId::Romm => "romm",
        TaskId::Artwork => "artwork",
    }
}

/// Counts up to `target` instead of jumping, unless motion is reduced.
fn use_count_up(target: u64) -> u64 {
    let mut shown = use_signal(|| target);
    let mut climbing = use_signal(|| None::<Task>);
    // Tests see the total at once, as the webview's do without frames.
    let reduced = use_hook(|| cfg!(test) || crate::theme::prefers_reduced_motion());
    use_effect(use_reactive!(|target| {
        if let Some(task) = climbing.write().take() {
            task.cancel();
        }
        let start = *shown.peek();
        if start == target {
            return;
        }
        if reduced {
            shown.set(target);
            return;
        }
        let began = Instant::now();
        let task = spawn(async move {
            loop {
                sleep(COUNT_FRAME).await;
                let t = (began.elapsed().as_secs_f64() / COUNT_UP.as_secs_f64()).min(1.0);
                let eased = 1.0 - (1.0 - t).powi(3);
                let value = start as f64 + (target as f64 - start as f64) * eased;
                shown.set(value.round().max(0.0) as u64);
                if t >= 1.0 {
                    break;
                }
            }
        });
        climbing.set(Some(task));
    }));
    shown()
}

/// What the screen shares with a run: whether it's still shown, which
/// attempt it's on, and when a task last changed.
struct Live {
    mounted: Cell<bool>,
    attempt: Cell<u32>,
    changed: Cell<Instant>,
}

/// A run's reports, for the attempt that started it while the screen is
/// shown. The run itself owns its parts and goes on when the user opens
/// the library; only the screen's updates and sounds stop.
struct Progress {
    live: Rc<Live>,
    attempt: u32,
    tasks: Signal<Vec<super::run::Task>>,
    ready: Signal<bool>,
    quiet: Signal<u64>,
    sounds: Sounds,
}

impl Progress {
    fn active(&self) -> bool {
        self.live.mounted.get() && self.live.attempt.get() == self.attempt
    }
}

impl Report for Progress {
    fn update(&self, next: Vec<super::run::Task>) {
        if !self.active() {
            return;
        }
        self.live.changed.set(Instant::now());
        let (mut quiet, mut tasks) = (self.quiet, self.tasks);
        quiet.set(0);
        tasks.set(next);
    }

    fn cue(&self, cue: Cue) {
        if self.active() {
            self.sounds.play(cue);
        }
    }

    fn ready(&self) {
        if self.active() {
            let mut ready = self.ready;
            ready.set(true);
        }
    }
}

#[component]
pub fn SetupStep(choices: Choices, sounds: Sounds, on_done: EventHandler<()>) -> Element {
    let settings = crate::data::use_settings();
    let discovery = crate::data::try_use_discovery();
    let emulation =
        try_use_context::<Emulation>().or_else(|| crate::shell::services().map(Emulation::of));
    let romm = crate::settings::use_romm();
    let tasks = use_signal(|| planned_tasks(&choices));
    let mut finished = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let ready = use_signal(|| false);
    let mut attempt = use_signal(|| 0u32);
    let mut elapsed = use_signal(|| 0u64);
    let mut quiet = use_signal(|| 0u64);
    let live = use_hook(|| {
        Rc::new(Live {
            mounted: Cell::new(true),
            attempt: Cell::new(0),
            changed: Cell::new(Instant::now()),
        })
    });
    // What a run starts with: this render's, as the webview's `latest`.
    let latest = use_hook(|| Rc::new(std::cell::RefCell::new(None)));
    *latest.borrow_mut() = Some((
        SetupParts {
            settings,
            discovery,
            emulation,
            romm,
        },
        choices.clone(),
        sounds.clone(),
    ));
    {
        let live = live.clone();
        use_drop(move || live.mounted.set(false));
    }
    {
        let live = live.clone();
        let latest = latest.clone();
        use_effect(move || {
            let attempt = attempt();
            let Some((parts, choices, sounds)) = latest.borrow().clone() else {
                return;
            };
            live.attempt.set(attempt);
            live.changed.set(Instant::now());
            // Only when they change: setting a signal runs its readers again.
            if error.peek().is_some() {
                error.set(None);
            }
            if *finished.peek() {
                finished.set(false);
            }
            let progress = Progress {
                live: live.clone(),
                attempt,
                tasks,
                ready,
                quiet,
                sounds: sounds.clone(),
            };
            // The run keeps going when the user opens the library: not the
            // screen's task, which ends with it.
            spawn_forever(async move {
                let ran = run_setup(&parts, &choices, &progress).await;
                if !progress.active() {
                    return;
                }
                match ran {
                    Ok(_) => {
                        sounds.play(Cue::Done);
                        finished.set(true);
                    }
                    Err(report) => error.set(Some(report.summary)),
                }
            });
        });
    }
    {
        let live = live.clone();
        use_effect(move || {
            if finished() {
                return;
            }
            let live = live.clone();
            let began = Instant::now();
            spawn(async move {
                loop {
                    sleep(TICK).await;
                    if *finished.peek() {
                        break;
                    }
                    elapsed.set(began.elapsed().as_secs());
                    quiet.set(live.changed.get().elapsed().as_secs());
                }
            });
        });
    }

    let mut stats = use_library_stats();
    let total_now = match &*stats.read() {
        Some(Ok(stats)) => stats.total,
        _ => 0,
    };
    let total = use_count_up(total_now);
    let mut games = use_query(DataKey::Games, |library| async move {
        let query = ListGames {
            filter: GameFilter::All,
            sort: GameSort::Recent,
            limit: 48,
            ..ListGames::default()
        };
        library.page(&query).await
    });
    let covers: Vec<(String, Option<String>)> = match &*games.read() {
        Some(Ok(page)) => page
            .items
            .iter()
            .filter(|game| game.cover.is_some())
            .take(24)
            .map(|game| {
                (
                    game.id.clone(),
                    crate::library::art_url_card(game.cover.as_ref()),
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    // Reads the count and the covers again while the work runs; library
    // events read them again too where the core sends them.
    use_effect(move || {
        if finished() {
            return;
        }
        spawn(async move {
            loop {
                sleep(REFRESH).await;
                if *finished.peek() {
                    break;
                }
                stats.restart();
                games.restart();
            }
        });
    });

    let is_finished = finished();
    let is_ready = ready();
    let failure = error();
    let quiet_for = quiet();
    let shown_tasks: Vec<(usize, &'static str, super::run::Task)> = tasks()
        .into_iter()
        .enumerate()
        .map(|(i, task)| (i, task_name(task.id), task))
        .collect();
    let failed = shown_tasks
        .iter()
        .any(|(_, _, task)| matches!(task.state, TaskState::Failed | TaskState::Warning));
    let heading = if failure.is_some() {
        t!("Setup needs attention")
    } else if is_finished {
        t!("You're all set")
    } else {
        t!("Setting things up")
    };
    let mut status = if is_ready {
        if is_finished {
            t!("Discovery has finished. Review any warnings below, or open your library.")
        } else {
            t!(
                "Games found so far are ready to play. More may appear while discovery continues in the background."
            )
        }
    } else if failure.is_some() {
        t!(
            "Your choices could not be saved. Try again to finish setup, or continue with your current settings."
        )
    } else {
        t!("Saving your choices before opening the library…")
    };
    if !is_finished {
        status.push(' ');
        status.push_str(&t!(
            "Elapsed: {seconds} seconds",
            seconds = format::count(elapsed() as f64)
        ));
    }
    let slow = quiet_for >= QUIET && !is_finished;
    let slow_text = if is_ready {
        t!(
            "No new progress reported for {seconds} seconds. A scan or network request may be taking longer. Open your library now; you can check or retry discovery in Settings → Library, and ROMs or RomM in Settings.",
            seconds = format::count(quiet_for as f64)
        )
    } else {
        t!(
            "Saving is taking longer than expected. Continue with your current settings, or wait. You can run setup again from Settings → General."
        )
    };

    rsx! {
        section {
            "data-part": "setup",
            "data-status": if is_finished { "done" } else { "running" },
            class: "relative",
            div {
                "data-part": "covers",
                "aria-hidden": "true",
                class: "pointer-events-none fixed inset-0 -z-10 grid grid-cols-6 content-center gap-3 p-6 opacity-25 [mask-image:radial-gradient(ellipse_at_center,transparent_25%,black_75%)]",
                for (i, (id, image)) in covers.into_iter().enumerate() {
                    div {
                        key: "{id}",
                        style: "--i: {i % 12}",
                        class: "aspect-[3/4] animate-rise overflow-hidden rounded-card",
                        GameArt { image, class: "size-full" }
                    }
                }
            }

            div { class: "mb-8 text-center",
                h1 { class: "animate-rise font-display text-3xl font-semibold", "{heading}" }
                p {
                    "data-part": "count",
                    "aria-live": "polite",
                    class: "mt-6 animate-rise font-display text-6xl font-semibold text-accent-alt tabular-nums [--i:2]",
                    "{total}"
                }
                p { class: "mt-1 animate-rise text-fg-muted [--i:3]",
                    {tn!(
                        note = "Under the library's game count, shown large above it",
                        "game in your library",
                        "games in your library",
                        total
                    )}
                }
            }

            p { role: "status", class: "mb-4 text-center text-sm text-fg-muted", "{status}" }
            if slow {
                p { role: "status", class: "mb-4 text-sm text-warning", "{slow_text}" }
            }
            ul { "data-part": "tasks", class: "flex flex-col gap-2",
                for (i, name, task) in shown_tasks {
                    li {
                        key: "{name}",
                        "data-part": "task",
                        "data-task": name,
                        "data-status": task.state.name(),
                        style: "--i: {i + 4}",
                        class: "relative flex animate-rise items-center gap-3 overflow-hidden rounded-panel border border-border bg-surface-raised px-4 py-3",
                        Svg { icon: icon(task.state), class: format!("size-5 shrink-0 {}", tone(task.state)) }
                        span { class: "font-medium", "{task.label}" }
                        span { class: "ml-auto min-w-0 text-right text-sm break-words text-fg-muted", "{task.detail}" }
                        if task.state == TaskState::Running {
                            span {
                                "aria-hidden": "true",
                                class: "absolute bottom-0 left-0 h-0.5 w-2/5 animate-progress bg-linear-to-r from-transparent via-accent-alt to-transparent [animation-play-state:var(--cs-motion-ambient)]",
                            }
                        }
                    }
                }
            }

            if let Some(message) = failure.clone() {
                p { role: "alert", class: "mt-4 text-center text-sm text-danger", "{message}" }
            }
            if !is_ready && (failure.is_some() || slow) {
                Button {
                    id: "onboarding-continue-current",
                    on_press: move |_| on_done.call(()),
                    {t!("Continue with current settings")}
                }
            }
            if failure.is_some() && !is_ready {
                Button {
                    id: "onboarding-setup-retry",
                    on_press: move |_| *attempt.write() += 1,
                    {t!("Try again")}
                }
            }
            if is_ready {
                div { class: "mt-10 flex animate-rise flex-col items-center gap-3",
                    Button {
                        id: "onboarding-open-library",
                        "data-part": "next",
                        "data-focus-default": "true",
                        class: OPEN,
                        on_press: move |_| on_done.call(()),
                        {t!("Open my library")}
                    }
                    if !is_finished {
                        p { class: "text-sm text-fg-muted",
                            {t!("Setup will keep running in the background.")}
                        }
                    }
                    if failed {
                        p { class: "text-sm text-fg-subtle",
                            {t!("Retry discovery in Settings → Library, ROM folders in Settings → Emulation, or RomM in Settings → RomM.")}
                        }
                    }
                }
            }
        }
    }
}
