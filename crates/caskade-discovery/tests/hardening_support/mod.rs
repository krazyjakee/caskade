//! Aggregate-only report invariants; never log private game metadata.
#![allow(dead_code)]
use caskade_discovery::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Default, Serialize)]
pub struct Invariants {
    pub groups: usize,
    pub records: usize,
    pub advertised_certain_dirs: usize,
    pub empty_groups: usize,
    pub missing_certain_dirs: usize,
    pub missing_certain_by_source: BTreeMap<String, usize>,
    pub duplicate_external_ids: usize,
    pub launcher_or_redist_groups: usize,
    pub failed_sources: usize,
}
impl Invariants {
    pub fn healthy(&self) -> bool {
        self.empty_groups == 0
            && self.missing_certain_dirs == 0
            && self.duplicate_external_ids == 0
            && self.launcher_or_redist_groups == 0
            && self.failed_sources == 0
    }
}
pub fn check(env: &HostEnv, report: &ScanReport) -> Invariants {
    let data = data::Data::builtin().unwrap();
    let redists = caskade_installer::redist::RedistTable::default();
    let mut result = Invariants {
        groups: report.groups.len(),
        records: report.groups.iter().map(|g| g.entries.len()).sum(),
        failed_sources: report.sources.iter().filter(|s| s.error.is_some()).count(),
        ..Default::default()
    };
    let mut global = BTreeSet::new();
    for group in &report.groups {
        let Some(primary) = group.primary_entry() else {
            result.empty_groups += 1;
            continue;
        };
        if primary.confidence == Confidence::Certain
            && let Some(dir) = &primary.install_dir
        {
            result.advertised_certain_dirs += 1;
            if !env.resolve_path(dir).is_ok_and(|path| path.is_dir()) {
                result.missing_certain_dirs += 1;
                *result
                    .missing_certain_by_source
                    .entry(primary.source.to_string())
                    .or_default() += 1;
            }
        }
        let ids: BTreeSet<_> = group
            .entries
            .iter()
            .flat_map(|game| game.ids.iter().flat_map(uri::external_identities))
            .filter_map(|id| match id {
                uri::UriIdentity::External(id) => Some(id),
                _ => None,
            })
            .collect();
        for id in ids {
            if !global.insert(id) {
                result.duplicate_external_ids += 1;
            }
        }
        let title = primary.title.trim().to_lowercase();
        let excluded_rule=primary.evidence.iter().any(|e|matches!(e,Evidence::Rule{rule,..}if matches!(rule.as_str(),"category.launcher"|"category.emulator"|"category.redist"|"category.driver"|"category.sdk")));
        let launcher = data
            .launchers
            .launchers
            .iter()
            .any(|launcher| title == launcher.name.to_lowercase());
        let redist = redists
            .redists
            .iter()
            .flat_map(|redist| &redist.product_names)
            .any(|product| title.starts_with(&product.to_lowercase()));
        let tool=primary.ids.iter().any(|id|matches!(id,ExternalId::Steam{app_id}if data.steam_tools.tools.iter().any(|tool|tool.app_id==*app_id)));
        if excluded_rule || launcher || redist || tool {
            result.launcher_or_redist_groups += 1;
        }
    }
    result
}
