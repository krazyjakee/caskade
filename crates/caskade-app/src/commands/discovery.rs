//! Controller operations, with no client-authored discovery records.
use crate::discovery::{DiscoveryController, DiscoverySnapshot};
use crate::error::CommandError;
pub async fn discovery_status(
    controller: &DiscoveryController,
) -> Result<DiscoverySnapshot, CommandError> {
    Ok(controller.snapshot().await)
}

pub async fn discovery_launchers(
    controller: &DiscoveryController,
) -> Result<DiscoverySnapshot, CommandError> {
    controller.launchers().await
}

pub async fn discovery_scan(
    controller: &DiscoveryController,
) -> Result<DiscoverySnapshot, CommandError> {
    crate::permissions::enforce("discovery_scan", serde_json::Value::default)?;
    controller.scan().await
}

pub async fn discovery_cancel(
    controller: &DiscoveryController,
) -> Result<DiscoverySnapshot, CommandError> {
    crate::permissions::enforce("discovery_cancel", serde_json::Value::default)?;
    Ok(controller.cancel().await)
}

pub async fn discovery_review(
    controller: &DiscoveryController,
    ids: Vec<String>,
    accept: bool,
    revision: u32,
) -> Result<DiscoverySnapshot, CommandError> {
    crate::permissions::enforce("discovery_review", serde_json::Value::default)?;
    controller.review(ids, accept, revision).await
}
