//! A scan's results to review before they're added (the webview's
//! `ScanReview`).

use super::*;
use std::rc::Rc;

/// One game of the review list as the user is shaping it; `key` stays
/// stable across edits.
#[derive(Clone, PartialEq)]
pub(super) struct Item {
    key: String,
    roms: Vec<String>,
    names: BTreeMap<String, Option<String>>,
    include: bool,
    title: String,
    platform: String,
    regions: Vec<String>,
}

pub(super) fn items(job: &ScanJob) -> Vec<Item> {
    job.review
        .iter()
        .map(|game| Item {
            key: game
                .roms
                .first()
                .map_or_else(|| game.title.clone(), |rom| rom.path.clone()),
            roms: game.roms.iter().map(|rom| rom.path.clone()).collect(),
            names: game
                .roms
                .iter()
                .map(|rom| (rom.path.clone(), rom.name.clone()))
                .collect(),
            include: true,
            title: game.title.clone(),
            platform: game.platform.clone(),
            regions: game.regions.clone(),
        })
        .collect()
}

/// The manual-scan review list: untick, fix titles, platforms and regions
/// in bulk, merge or split multi-disc games. The choices are stored, so
/// later automatic scans agree.
#[component]
pub(super) fn ScanReview(
    job: ScanJob,
    platforms: Vec<PlatformView>,
    on_done: EventHandler<()>,
) -> Element {
    let emulation = use_emulation();
    let live = use_live();
    let mut rows = use_signal(|| items(&job));
    let mut selected = use_signal(BTreeSet::<String>::new);
    let mut bulk_region = use_signal(String::new);
    let mut submitting = use_signal(|| false);
    let mut cancelling = use_signal(|| false);
    let mut submit_error = use_signal(|| None::<ErrorReport>);
    let mut cancel_error = use_signal(|| None::<ErrorReport>);
    let done = move |next: ScanJob| {
        let mut scans = live.scans;
        merge_scan(&mut scans.write(), next);
        on_done.call(());
    };
    let submit = {
        let emulation = emulation.clone();
        let (id, revision) = (job.id.clone(), job.revision);
        move |_| {
            submitting.set(true);
            submit_error.set(None);
            let choices: Vec<ReviewChoice> = rows
                .peek()
                .iter()
                .map(|row| ReviewChoice {
                    roms: row.roms.clone(),
                    include: row.include,
                    title: row.title.trim().to_owned(),
                    platform: row.platform.clone(),
                    regions: row.regions.clone(),
                })
                .collect();
            let emulation = emulation.clone();
            let id = id.clone();
            spawn(async move {
                let result =
                    change(async move { emulation.submit_review(&id, revision, choices).await })
                        .await;
                submitting.set(false);
                match result {
                    Ok(next) => done(next),
                    Err(report) => submit_error.set(Some(report)),
                }
            });
        }
    };
    let cancel = {
        let emulation = emulation.clone();
        let id = job.id.clone();
        move |_| {
            cancelling.set(true);
            cancel_error.set(None);
            let emulation = emulation.clone();
            let id = id.clone();
            spawn(async move {
                let result = change(async move { emulation.cancel_scan(&id) }).await;
                cancelling.set(false);
                match result {
                    Ok(next) => done(next),
                    Err(report) => cancel_error.set(Some(report)),
                }
            });
        }
    };
    // Read in place, not cloned: a review can hold thousands of ROMs (#111).
    let all = rows.read();
    let chosen = selected();
    let picked: Vec<Item> = all
        .iter()
        .filter(|row| chosen.contains(&row.key))
        .cloned()
        .collect();
    let merge = {
        let picked = picked.clone();
        move |_| {
            let Some(first) = picked.first().cloned() else {
                return;
            };
            let rest: BTreeSet<String> = picked[1..].iter().map(|item| item.key.clone()).collect();
            let next: Vec<Item> = rows
                .peek()
                .iter()
                .filter(|row| !rest.contains(&row.key))
                .map(|row| {
                    if row.key == first.key {
                        Item {
                            roms: picked.iter().flat_map(|item| item.roms.clone()).collect(),
                            names: picked.iter().flat_map(|item| item.names.clone()).collect(),
                            ..row.clone()
                        }
                    } else {
                        row.clone()
                    }
                })
                .collect();
            rows.set(next);
            selected.set(BTreeSet::new());
        }
    };
    let split = move |item: Item| {
        let next: Vec<Item> = rows
            .peek()
            .iter()
            .flat_map(|row| {
                if row.key == item.key {
                    row.roms
                        .iter()
                        .map(|rom| Item {
                            key: rom.clone(),
                            roms: vec![rom.clone()],
                            ..row.clone()
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![row.clone()]
                }
            })
            .collect();
        rows.set(next);
    };
    let included = all.iter().filter(|row| row.include).count();
    let busy = submitting() || cancelling();
    let untitled = all
        .iter()
        .any(|row| row.include && row.title.trim().is_empty());
    // One list every game's platform picker shares.
    let platform_options: Rc<Vec<SelectOption>> = Rc::new(
        platforms
            .iter()
            .map(|platform| {
                SelectOption::new(platform.platform.id.clone(), platform.platform.name.clone())
            })
            .collect(),
    );
    let failure = submit_error().or(cancel_error());
    // Each game's place, key and whether it's selected.
    let games: Vec<(usize, String, bool, Item)> = all
        .iter()
        .enumerate()
        .map(|(index, row)| {
            (
                index,
                row.key.clone(),
                chosen.contains(&row.key),
                row.clone(),
            )
        })
        .collect();
    let total = all.len();
    drop(all);
    rsx! {
        SettingsSection {
            id: "scan-review",
            icon: LIST_CHECKS,
            title: t!("Review scan results"),
            description: t!("Untick what isn't a game, fix titles, platforms and regions, and merge or split discs. Your choices are kept for later automatic scans."),
            div { class: "mt-3 space-y-3",
                for warning in job.warnings.iter() {
                    p { key: "{warning}", class: "text-xs text-warning", "{warning}" }
                }
                if total == 0 {
                    p { class: "rounded-control border border-dashed border-border p-5 text-sm text-fg-muted",
                        {t!("No ROMs matched this scanner's profile.")}
                    }
                } else {
                    div {
                        "data-part": "bulk",
                        class: "flex flex-wrap items-end gap-2 rounded-control border border-border bg-surface-overlay/40 p-3",
                        Checkbox {
                            id: "review-select-all",
                            label: t!("Select all"),
                            checked: picked.len() == total,
                            indeterminate: !picked.is_empty() && picked.len() < total,
                            on_change: move |on: bool| {
                                let next = if on {
                                    rows.peek().iter().map(|row| row.key.clone()).collect()
                                } else {
                                    BTreeSet::new()
                                };
                                selected.set(next);
                            },
                        }
                        span { class: "text-xs text-fg-muted",
                            {t!(note = "How many games of the scan review are selected", "{0} selected", 0 = picked.len())}
                        }
                        Button {
                            id: "review-merge",
                            size: ButtonSize::Sm,
                            icon: COMBINE,
                            disabled: picked.len() < 2,
                            on_press: merge,
                            {t!("Merge into one game")}
                        }
                        Select {
                            id: "review-platform",
                            "aria-label": t!("Set platform of selected games"),
                            placeholder: t!("Set platform…"),
                            options: platform_options.to_vec(),
                            value: None,
                            disabled: picked.is_empty(),
                            on_change: move |platform: String| {
                                if platform.is_empty() {
                                    return;
                                }
                                let chosen = selected.peek().clone();
                                for row in rows.write().iter_mut() {
                                    if chosen.contains(&row.key) {
                                        row.platform = platform.clone();
                                    }
                                }
                            },
                        }
                        div { class: "w-44",
                            TextField {
                                id: "review-regions",
                                "aria-label": t!("Regions for selected games"),
                                placeholder: t!(
                                    note = "Regions are comma-separated region codes; keep the example code",
                                    "Regions, e.g. usa"
                                ),
                                value: bulk_region(),
                                on_input: move |next: String| bulk_region.set(next),
                            }
                        }
                        Button {
                            id: "review-set-regions",
                            size: ButtonSize::Sm,
                            disabled: picked.is_empty(),
                            on_press: move |_| {
                                let regions: Vec<String> = bulk_region
                                    .peek()
                                    .split(',')
                                    .map(|region| region.trim().to_owned())
                                    .filter(|region| !region.is_empty())
                                    .collect();
                                let chosen = selected.peek().clone();
                                for row in rows.write().iter_mut() {
                                    if chosen.contains(&row.key) {
                                        row.regions = regions.clone();
                                    }
                                }
                            },
                            {t!("Set regions")}
                        }
                    }
                    ul { class: "space-y-2",
                        for (index, key, picked, row) in games {
                            ReviewGame {
                                key: "{key}",
                                index,
                                selected: picked,
                                options: platform_options.clone(),
                                on_select: {
                                    let key = key.clone();
                                    move |on: bool| {
                                        let mut chosen = selected.write();
                                        if on {
                                            chosen.insert(key.clone());
                                        } else {
                                            chosen.remove(&key);
                                        }
                                    }
                                },
                                on_update: move |next: Item| {
                                    if let Some(slot) = rows.write().iter_mut().find(|row| row.key == next.key) {
                                        *slot = next;
                                    }
                                },
                                on_split: split,
                                item: row,
                            }
                        }
                    }
                }
                if let Some(report) = failure {
                    p { role: "alert", class: "text-sm text-danger", "{report.summary}" }
                }
                div { class: "flex justify-end gap-2",
                    Button { id: "review-discard", disabled: busy, on_press: cancel, {t!("Discard review")} }
                    Button {
                        id: "review-import",
                        variant: ButtonVariant::Primary,
                        disabled: busy || untitled,
                        on_press: submit,
                        if submitting() {
                            {t!("Importing…")}
                        } else {
                            {tn!("Import # game", "Import # games", included)}
                        }
                    }
                }
            }
        }
    }
}

/// One game of the review list.
#[component]
pub(super) fn ReviewGame(
    item: Item,
    index: usize,
    selected: bool,
    options: Rc<Vec<SelectOption>>,
    on_select: EventHandler<bool>,
    on_update: EventHandler<Item>,
    on_split: EventHandler<Item>,
) -> Element {
    let first = item.roms.first().cloned().unwrap_or_default();
    let roms: Vec<String> = item
        .roms
        .iter()
        .map(|rom| match item.names.get(rom) {
            Some(Some(name)) if !name.is_empty() => format!("{rom} · {name}"),
            _ => rom.clone(),
        })
        .collect();
    rsx! {
        li {
            "data-part": "review-game",
            "data-included": if item.include { "true" } else { "false" },
            class: "grid gap-3 rounded-control border border-border p-3 data-[included=false]:opacity-60 md:grid-cols-[auto_minmax(0,1fr)_14rem_auto]",
            div { class: "flex flex-col gap-2",
                Checkbox {
                    id: "review-select-{index}",
                    "aria-label": t!("Select {title}", title = item.title.clone()),
                    checked: selected,
                    on_change: move |on: bool| on_select.call(on),
                }
            }
            div { class: "min-w-0 space-y-2",
                TextField {
                    id: "review-title-{index}",
                    "aria-label": t!("Title of {rom}", rom = first),
                    value: item.title.clone(),
                    on_input: {
                        let item = item.clone();
                        move |title: String| on_update.call(Item { title, ..item.clone() })
                    },
                }
                ol {
                    "aria-label": t!("ROMs of {title}", title = item.title.clone()),
                    class: "space-y-0.5",
                    for (rom, line) in item.roms.iter().zip(roms) {
                        li { key: "{rom}", class: "break-all font-mono text-xs text-fg-muted", "{line}" }
                    }
                }
                if !item.regions.is_empty() {
                    p { class: "text-xs text-fg-muted",
                        {t!("Regions: {regions}", regions = item.regions.join(", "))}
                    }
                }
            }
            Select {
                id: "review-platform-{index}",
                "aria-label": t!("Platform of {title}", title = item.title.clone()),
                options: options.to_vec(),
                value: Some(item.platform.clone()),
                on_change: {
                    let item = item.clone();
                    move |platform: String| on_update.call(Item { platform, ..item.clone() })
                },
            }
            div { class: "flex flex-col items-end gap-2",
                Checkbox {
                    id: "review-include-{index}",
                    label: t!(note = "Import this game from the scan", "Import"),
                    checked: item.include,
                    on_change: {
                        let item = item.clone();
                        move |include: bool| on_update.call(Item { include, ..item.clone() })
                    },
                }
                if item.roms.len() > 1 {
                    Button {
                        id: "review-split-{index}",
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        icon: SPLIT,
                        on_press: {
                            let item = item.clone();
                            move |_| on_split.call(item.clone())
                        },
                        {t!("Split discs")}
                    }
                }
            }
        }
    }
}
