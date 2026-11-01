use super::{FlatpakMetadata, metadata_from_artifact};
use crate::exec::{
    CancellationToken, ExecError, ExecReport, ExecRequest, Executor, FinalizationContext, Finalizer,
};
use crate::manifest::{
    HandlerVersion, InstallManifest, InstallRecord, ManifestSource, RelocationRecord,
    RelocationVerdict,
};
use crate::{ArtifactNode, InstallOutcome, InstallPlan, RuntimeSpec, Step, Strategy};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone)]
pub struct FlatpakInstaller {
    executor: Executor,
}
pub struct FlatpakInstallReport {
    pub execution: ExecReport,
    pub outcome: InstallOutcome,
    pub manifest: InstallManifest,
}
impl FlatpakInstaller {
    pub fn new(executor: Executor) -> Self {
        Self { executor }
    }
    pub async fn install(
        &self,
        request: ExecRequest,
        artifact: ArtifactNode,
        source: ManifestSource,
        root_id: String,
        cancellation: CancellationToken,
    ) -> crate::exec::Result<FlatpakInstallReport> {
        let metadata = metadata_from_artifact(&artifact)
            .filter(|metadata| metadata.application)
            .ok_or_else(|| invalid("missing Flatpak application identity"))?;
        if request.plans.candidates.iter().any(|candidate| {
            candidate.handler.0 != "flatpak" || candidate.strategy != Strategy::Delegate
        }) {
            return Err(invalid("expected Flatpak delegate candidates"));
        }
        let finalizer = Arc::new(Record {
            artifact,
            source,
            root_id,
            metadata: metadata.clone(),
        });
        let execution = self
            .executor
            .clone()
            .with_finalizer(finalizer)
            .execute(request, cancellation)
            .await?;
        let manifest =
            InstallManifest::read(&execution.dir).map_err(|error| invalid(error.to_string()))?;
        let tool = execution
            .candidate
            .steps
            .iter()
            .find_map(|step| {
                if let Step::Delegate { tool, .. } = step {
                    Some(tool.as_str())
                } else {
                    None
                }
            })
            .ok_or_else(|| invalid("missing Flatpak delegate"))?;
        let mut outcome = metadata.handoff(tool);
        if let InstallOutcome::HandOff(crate::HandOff::Delegated {
            uninstall_steps, ..
        }) = &mut outcome
        {
            *uninstall_steps = manifest.record().uninstall_steps.clone();
        }
        Ok(FlatpakInstallReport {
            execution,
            outcome,
            manifest,
        })
    }
}
fn invalid(message: impl Into<String>) -> ExecError {
    ExecError::InvalidRequest {
        message: message.into(),
    }
}
struct Record {
    artifact: ArtifactNode,
    source: ManifestSource,
    root_id: String,
    metadata: FlatpakMetadata,
}
impl Finalizer for Record {
    fn finalize(&self, context: &FinalizationContext<'_>) -> crate::exec::Result<()> {
        context.steps.checkpoint(0)?;
        let mut uninstall_steps = vec![];
        for step in &context.candidate.steps {
            if let Step::Delegate { uninstall, .. } = step {
                for command in uninstall {
                    uninstall_steps.push(Step::Delegate {
                        tool: command.tool.clone(),
                        args: command.args.clone(),
                        uninstall: vec![],
                        only_if: command.only_if.clone(),
                    });
                }
            }
        }
        let manifest = InstallManifest::new(InstallRecord {
            title: self
                .metadata
                .title
                .clone()
                .unwrap_or_else(|| self.metadata.id.clone()),
            ids: BTreeMap::from([("flatpak".into(), self.metadata.id.clone())]),
            source: self.source.clone(),
            plan: InstallPlan {
                schema_version: 1,
                input: self.source.input.clone(),
                artifacts: vec![self.artifact.clone()],
                root_id: self.root_id.clone(),
                relative_dir: context.relative_dir.into(),
                candidate: context.candidate.clone(),
            },
            handlers: vec![HandlerVersion {
                id: context.candidate.handler.clone(),
                version: env!("CARGO_PKG_VERSION").into(),
            }],
            files: vec![],
            launch_targets: vec![],
            runtime: RuntimeSpec::Native,
            uninstall_steps,
            external_effects: vec![],
            relocation: RelocationRecord {
                verdict: RelocationVerdict::Pinned,
                ..Default::default()
            },
            alias: None,
        })
        .map_err(|error| invalid(error.to_string()))?;
        manifest
            .write(context.game_dir)
            .map_err(|error| invalid(error.to_string()))?;
        Ok(())
    }
}
