//! Interceptors: an ordered chain over one payload type per hook point
//! (TECH-PLAN §4.3). Each step may rewrite the payload or cancel; a step
//! that fails, panics or runs past its time box is skipped and logged, and
//! the chain continues with the payload it had.

use async_trait::async_trait;
use std::{any::Any, sync::Arc, time::Duration};

/// A hook point: its stable name and the payload its interceptors see.
/// Points are plain types, so each module defines its own (`launch.before`
/// over `launch::LaunchCommand`, …).
pub trait InterceptPoint: Send + Sync + 'static {
    type Payload: Clone + Send + Sync + 'static;
    const NAME: &'static str;
}

/// What one interceptor decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intercept<T> {
    /// Go on with this (possibly rewritten) payload.
    Continue(T),
    /// Stop the operation.
    Cancel(String),
}

#[async_trait]
pub trait Interceptor<P: InterceptPoint>: Send + Sync {
    /// Cold backend preparation is separate from the call's execution budget.
    async fn prepare(&self) -> Result<(), String> {
        Ok(())
    }
    async fn intercept(&self, payload: P::Payload) -> Result<Intercept<P::Payload>, String>;
}

/// The result of running a point's chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    Continue(T),
    Cancelled { by: String, reason: String },
}

/// The chain's result with the steps that were skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterceptReport<T> {
    pub outcome: Outcome<T>,
    /// `(interceptor id, why)` for every failed, panicked or timed-out step.
    pub skipped: Vec<(String, String)>,
}

pub(super) struct Registration<P: InterceptPoint> {
    pub id: String,
    pub priority: i32,
    pub seq: u64,
    pub interceptor: Arc<dyn Interceptor<P>>,
}

/// The registrations of one point, stored type-erased in the bus.
pub(super) struct Chain<P: InterceptPoint> {
    pub steps: Vec<Registration<P>>,
}

impl<P: InterceptPoint> Chain<P> {
    pub fn insert(&mut self, registration: Registration<P>) {
        self.steps.push(registration);
        self.steps.sort_by_key(|step| (step.priority, step.seq));
    }

    pub fn snapshot(&self) -> Vec<(String, Arc<dyn Interceptor<P>>)> {
        self.steps
            .iter()
            .map(|step| (step.id.clone(), step.interceptor.clone()))
            .collect()
    }
}

/// A chain of any point, so steps can be removed by id across points.
pub(super) trait AnyChain: Send + Sync {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Removes the steps registered as `id`; returns how many.
    fn remove(&mut self, id: &str) -> usize;
}

impl<P: InterceptPoint> AnyChain for Chain<P> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn remove(&mut self, id: &str) -> usize {
        let before = self.steps.len();
        self.steps.retain(|step| step.id != id);
        before - self.steps.len()
    }
}

pub(super) fn downcast<P: InterceptPoint>(chain: &mut Box<dyn AnyChain>) -> &mut Chain<P> {
    chain
        .as_any_mut()
        .downcast_mut::<Chain<P>>()
        .expect("chain type matches its point")
}

/// Runs `steps` in order with a per-step time box.
pub(super) async fn run<P: InterceptPoint>(
    steps: Vec<(String, Arc<dyn Interceptor<P>>)>,
    mut payload: P::Payload,
    timeout: Duration,
) -> InterceptReport<P::Payload> {
    let mut skipped = Vec::new();
    for (id, interceptor) in steps {
        let input = payload.clone();
        // Waiting for a resource slot is not plugin execution time. Child
        // tasks inherit the slot, including calls that outlive this time box.
        let result = caskade_jobs::global()
            .run(crate::tr!("Updating plugins"), async move {
                let (prepared, ready) = tokio::sync::oneshot::channel();
                let mut task = tokio::spawn(caskade_jobs::inherit_async(async move {
                    interceptor.prepare().await?;
                    let _ = prepared.send(());
                    interceptor.intercept(input).await
                }));
                if tokio::time::timeout(super::PREPARATION_TIMEOUT, ready)
                    .await
                    .is_err()
                {
                    task.abort();
                    return Err("background hook preparation timed out".to_owned());
                }
                match tokio::time::timeout(timeout, &mut task).await {
                    Err(_) => {
                        // Stop the step, so a late interceptor has no side effects.
                        task.abort();
                        Err(format!("timed out after {} ms", timeout.as_millis()))
                    }
                    Ok(Err(join)) if join.is_panic() => Err("panicked".to_owned()),
                    Ok(Err(join)) => Err(join.to_string()),
                    Ok(Ok(Err(error))) => Err(error),
                    Ok(Ok(Ok(step))) => Ok(step),
                }
            })
            .await;
        match result {
            Ok(Intercept::Continue(next)) => payload = next,
            Ok(Intercept::Cancel(reason)) => {
                tracing::info!(point = P::NAME, interceptor = %id, %reason, "interceptor cancelled");
                return InterceptReport {
                    outcome: Outcome::Cancelled { by: id, reason },
                    skipped,
                };
            }
            Err(why) => {
                tracing::warn!(point = P::NAME, interceptor = %id, %why, "interceptor skipped");
                skipped.push((id, why));
            }
        }
    }
    InterceptReport {
        outcome: Outcome::Continue(payload),
        skipped,
    }
}
