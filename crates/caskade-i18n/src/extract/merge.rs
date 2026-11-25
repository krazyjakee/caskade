//! Merging POT templates into PO catalogs (`msgmerge` without fuzzy matching,
//! with optional same-language compendium catalogs).

use super::check::describe;
use crate::po::{Message, Po};
use std::collections::{HashMap, HashSet};

type Key<'a> = (Option<&'a str>, &'a str);

/// Has a translation a lookup would show: not fuzzy, a form not empty.
fn translated(message: &Message) -> bool {
    !message.is_fuzzy() && message.translations.iter().any(|text| !text.is_empty())
}

/// `catalog` updated to `template`, as `msgmerge` does without fuzzy
/// matching or a compendium: see [`merge_with`].
pub fn merge(template: &Po, catalog: &Po) -> Result<Po, String> {
    merge_with(template, catalog, &[])
}

/// `catalog` updated to `template`, as `msgmerge --compendium` does without
/// fuzzy matching:
///
/// - a message still in the template keeps its translation, comments and
///   flags (marked fuzzy when its English plural changed, or when it gained
///   or lost its plural, keeping the first form);
/// - an obsolete entry whose message is back is brought back;
/// - a new message, or one still untranslated, takes the translation a
///   `compendium` catalog (the same language's other domains) has for it,
///   else comes in untranslated;
/// - a translated message gone from the template becomes an obsolete
///   (`#~`) entry, keeping its translation, translator comments and fuzzy
///   flag; untranslated ones go.
///
/// References and notes for translators follow the template.
pub fn merge_with(template: &Po, catalog: &Po, compendium: &[Po]) -> Result<Po, String> {
    let forms = catalog.plural_forms().map_err(|error| error.to_string())?;
    let active: HashMap<Key<'_>, &Message> = catalog
        .messages
        .iter()
        .map(|message| (message.key(), message))
        .collect();
    let obsolete: HashMap<Key<'_>, &Message> = catalog
        .obsolete
        .iter()
        .map(|message| (message.key(), message))
        .collect();
    let mut memory: HashMap<Key<'_>, &Message> = HashMap::new();
    for found in compendium
        .iter()
        .flat_map(|po| po.messages.iter().chain(&po.obsolete))
    {
        let whole = found.plural.is_none() || found.translations.len() == forms.count;
        if translated(found) && whole {
            memory.entry(found.key()).or_insert(found);
        }
    }
    let messages = template
        .messages
        .iter()
        .map(|source| {
            let remembered = memory
                .get(&source.key())
                .filter(|found| found.plural.is_some() == source.plural.is_some());
            let mut merged = match active
                .get(&source.key())
                .or_else(|| obsolete.get(&source.key()))
            {
                Some(old) => {
                    let mut merged = (*old).clone();
                    if old.plural.is_some() != source.plural.is_some() {
                        let first = old.translations.first().cloned().unwrap_or_default();
                        merged.translations = if source.plural.is_some() {
                            vec![String::new(); forms.count]
                        } else {
                            vec![String::new()]
                        };
                        if let Some(slot) = merged.translations.first_mut() {
                            slot.clone_from(&first);
                        }
                        if !first.is_empty() && !merged.is_fuzzy() {
                            merged.flags.push("fuzzy".to_owned());
                        }
                    } else if old.plural != source.plural && !merged.is_fuzzy() {
                        merged.flags.push("fuzzy".to_owned());
                    }
                    let blank = merged.translations.iter().all(String::is_empty);
                    if blank
                        && !merged.is_fuzzy()
                        && let Some(found) = remembered
                    {
                        merged.translations.clone_from(&found.translations);
                    }
                    merged
                }
                None => {
                    let mut new = source.clone();
                    match remembered {
                        Some(found) => {
                            new.translations.clone_from(&found.translations);
                            new.translator_comments
                                .clone_from(&found.translator_comments);
                        }
                        None if source.plural.is_some() => {
                            new.translations = vec![String::new(); forms.count];
                        }
                        None => {}
                    }
                    new
                }
            };
            merged.plural.clone_from(&source.plural);
            merged.references.clone_from(&source.references);
            merged
                .extracted_comments
                .clone_from(&source.extracted_comments);
            merged
        })
        .collect();
    let current: HashSet<Key<'_>> = template.messages.iter().map(Message::key).collect();
    let mut seen = HashSet::new();
    let mut gone = Vec::new();
    for old in catalog.messages.iter().chain(&catalog.obsolete) {
        if current.contains(&old.key())
            || old.translations.iter().all(String::is_empty)
            || !seen.insert(old.key())
        {
            continue;
        }
        let mut entry = old.clone();
        entry.references.clear();
        entry.extracted_comments.clear();
        entry.flags.retain(|flag| flag == "fuzzy");
        gone.push(entry);
    }
    Ok(Po {
        header: catalog.header.clone().or_else(|| template.header.clone()),
        messages,
        obsolete: gone,
    })
}

/// The translations in `before` that `after` doesn't have: an entry with a
/// translation (fuzzy too) whose context and English no message or
/// obsolete entry of `after` has with a translation. A merge must never
/// lose one; a changed context would.
pub fn lost_translations(before: &Po, after: &Po) -> Vec<String> {
    let kept: HashSet<Key<'_>> = after
        .messages
        .iter()
        .chain(&after.obsolete)
        .filter(|entry| entry.translations.iter().any(|text| !text.is_empty()))
        .map(Message::key)
        .collect();
    before
        .messages
        .iter()
        .chain(&before.obsolete)
        .filter(|entry| {
            entry.translations.iter().any(|text| !text.is_empty()) && !kept.contains(&entry.key())
        })
        .map(describe)
        .collect()
}
