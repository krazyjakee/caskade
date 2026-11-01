//! The installer's hooks (#55, INSTALLER-PLAN §10): its interceptor points,
//! and the bus's install handlers and URL resolvers adapted to the
//! installer's own traits.

use crate::bus::{Bus, InstallHandler, InstallInput, InterceptPoint, Outcome};
use caskade_installer::{
    ArtifactId, ArtifactNode, Candidate, Warning,
    pipeline::{PlanReview, ReviewFuture},
    plan::{Handler, HandlerId, PlanContext, PlanError, PlanSet, Proposal},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

/// `install.plan`: the ranked plans before anything runs. An interceptor
/// may reorder, drop or select candidates, or cancel to stop the install.
pub struct InstallPlanned;
impl InterceptPoint for InstallPlanned {
    type Payload = PlannedInstall;
    const NAME: &'static str = "install.plan";
}

/// `install.after`: a finished install, for post-install tweaks. Rewrites
/// are ignored; a cancel becomes a warning on the job.
pub struct InstallAfter;
impl InterceptPoint for InstallAfter {
    type Payload = InstalledGame;
    const NAME: &'static str = "install.after";
}

/// `install.moved`: a game moved to another folder, so plugins can fix
/// references they own. Rewrites are ignored; a cancel becomes a warning.
pub struct InstallMoved;
impl InterceptPoint for InstallMoved {
    type Payload = MovedGame;
    const NAME: &'static str = "install.moved";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlannedInstall {
    pub job_id: String,
    pub artifact: ArtifactNode,
    pub plans: PlanSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstalledGame {
    pub job_id: String,
    pub game_id: String,
    pub title: String,
    /// The game's folder.
    pub dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MovedGame {
    pub job_id: String,
    pub game_id: String,
    pub from: PathBuf,
    pub to: PathBuf,
}

/// Runs `install.plan` on the bus for one job.
pub(super) struct BusReview {
    pub bus: Bus,
    pub job_id: String,
}

impl PlanReview for BusReview {
    fn review<'a>(&'a self, artifact: &'a ArtifactNode, plans: PlanSet) -> ReviewFuture<'a> {
        Box::pin(async move {
            let report = self
                .bus
                .run::<InstallPlanned>(PlannedInstall {
                    job_id: self.job_id.clone(),
                    artifact: artifact.clone(),
                    plans,
                })
                .await;
            match report.outcome {
                Outcome::Continue(planned) => Ok(planned.plans),
                Outcome::Cancelled { by, reason } => Err(format!("{by}: {reason}")),
            }
        })
    }
}

/// Runs a read-only point (`install.after`, `install.moved`); a cancel is
/// returned as a warning.
pub(super) async fn notify<P: InterceptPoint>(bus: &Bus, payload: P::Payload) -> Option<Warning> {
    match bus.run::<P>(payload).await.outcome {
        Outcome::Continue(_) => None,
        Outcome::Cancelled { by, reason } => Some(Warning {
            code: P::NAME.into(),
            message: format!("{by}: {reason}"),
            path: None,
        }),
    }
}

/// Asks every install handler on the bus about the input, before planning:
/// their calls are async and the plan engine is not. Each answer becomes a
/// handler the plan engine ranks with the built-ins.
pub(super) async fn propose(
    handlers: Vec<Arc<dyn InstallHandler>>,
    input: &InstallInput,
) -> Vec<Arc<dyn Handler>> {
    let mut proposed: Vec<Arc<dyn Handler>> = Vec::new();
    for handler in handlers {
        match handler.propose(input).await {
            Ok(Some(mut candidate)) => {
                candidate.handler = HandlerId(handler.id().to_owned());
                proposed.push(Arc::new(Proposed {
                    artifact: input.artifact.id.clone(),
                    candidate,
                }));
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(handler = handler.id(), %error, "install handler failed");
            }
        }
    }
    proposed
}

/// An install handler's answer as a plan-engine handler.
struct Proposed {
    artifact: ArtifactId,
    candidate: Candidate,
}

impl Handler for Proposed {
    fn id(&self) -> HandlerId {
        self.candidate.handler.clone()
    }
    fn claims(&self, artifact: &ArtifactNode) -> bool {
        artifact.id == self.artifact
    }
    fn propose(&self, _: &ArtifactNode, _: &PlanContext<'_>) -> Result<Proposal, PlanError> {
        Ok(Proposal {
            candidates: vec![self.candidate.clone()],
            question: None,
        })
    }
}

/// A bus URL resolver as an installer resolver; registered ones run before
/// the built-ins.
#[cfg(feature = "net")]
pub(super) struct BusResolver(pub Arc<dyn crate::bus::UrlResolver>);

#[cfg(feature = "net")]
impl caskade_installer::fetch::UrlResolver for BusResolver {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn claims(&self, url: &url::Url) -> bool {
        self.0.matches(url)
    }
    fn resolve<'a>(
        &'a self,
        url: &'a url::Url,
        _: &'a caskade_installer::host::HostCaps,
    ) -> caskade_installer::fetch::ResolveFuture<'a> {
        use caskade_installer::fetch::{DownloadRequest, FetchError, Resolution};
        Box::pin(async move {
            let found = self
                .0
                .resolve(url)
                .await
                .map_err(|message| FetchError::InvalidRequest { message })?;
            if found.is_empty() {
                return Err(FetchError::InvalidRequest {
                    message: crate::tr!("{plugin} found nothing to download", plugin = self.0.id()),
                });
            }
            Ok(Resolution::Downloads(
                found
                    .into_iter()
                    .map(|request| DownloadRequest {
                        url: request.url,
                        headers: request.headers,
                        file_name: request.file_name,
                        size: request.size,
                        sha256: request.sha256,
                        allowed_hosts: request.allowed_hosts,
                    })
                    .collect(),
            ))
        })
    }
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
