//! The harness of the codex contract scenarios: one codex thread on the fake Responses API.

use aae_codex::Codex;
use aae_core::{
    config::{EffectiveConfig, Layer, Sourced},
    driver::{
        Driver, DriverEvent, Failure, InstanceHandle, Normalized, PromptKey, SessionPlan, SessionRef, StopReason,
    },
};
use aae_testkit::openai::{Provider, Reply};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;

fn sourced<T>(value: T) -> Sourced<T> {
    Sourced {
        value,
        from: Layer::Agent,
    }
}

/// One codex thread on the fake with one prompt sent; `retries` is the provider's `request_max_retries`,
/// `approval` codex's approval policy.
pub(crate) struct Live {
    pub(crate) provider: Arc<Provider>,
    pub(crate) driver: Codex,
    pub(crate) instance: InstanceHandle,
    pub(crate) session: SessionRef,
    pub(crate) events: mpsc::UnboundedReceiver<DriverEvent>,
    pub(crate) seen: Vec<Normalized>,
    pub(crate) rig: PathBuf,
}

impl Live {
    pub(crate) async fn start(plan: Vec<Reply>, dir: &Path, retries: u32, approval: &str) -> Self {
        let provider = Arc::new(Provider::start(plan).await);
        Self::open(provider, dir, retries, approval, None).await.unwrap()
    }

    /// `resume` binds another live app-server's thread (a second writer): one provider and state, one `CODEX_HOME`.
    pub(crate) async fn open(
        provider: Arc<Provider>,
        dir: &Path,
        retries: u32,
        approval: &str,
        resume: Option<SessionRef>,
    ) -> Result<Self, Failure> {
        let id = if resume.is_some() { "i2" } else { "i1" };
        let driver = Codex::new("codex".into(), dir.join("state"), None, 8);
        let rig = dir.join("rig");
        std::fs::create_dir_all(&rig).unwrap();
        let (run, attempt, token) = ("r1".to_owned(), 1, "t".to_owned());
        let plan = match resume.clone() {
            None => SessionPlan::New {
                workdir: rig.clone(),
                run,
                attempt,
                token,
                actor: None,
            },
            Some(session) => SessionPlan::Resume {
                session,
                workdir: rig.clone(),
                run,
                attempt,
                token,
                actor: None,
            },
        };
        let native = json!({ "model_provider": "fake", "sandbox_mode": "danger-full-access", "model_providers": {
            "fake": { "name": "fake", "base_url": provider.base_url(), "wire_api": "responses",
            "request_max_retries": retries, "stream_max_retries": retries } } });
        let config = EffectiveConfig {
            services: Default::default(),
            harness: "codex".into(),
            model: Some(sourced("gpt-fake".to_owned())),
            effort: None,
            system_prompt: None,
            permission_mode: Some(sourced(approval.to_owned())),
            inherit_host: Some(sourced(false)),
            env: Default::default(),
            mcp: Default::default(),
            hooks: Default::default(),
            skills: Default::default(),
            plugins: Vec::new(),
            // The host forbids user namespaces, so the codex sandbox cannot run here (docs/design/experiments.md).
            native: Some(sourced(native)),
        };
        let split = driver.split(&config, &plan).unwrap();
        let (sink, events) = mpsc::unbounded_channel();
        let instance = driver.start_instance(id, &split.process, sink).await.unwrap();
        let session = match resume {
            None => driver.start_session(&instance, &split.session).await.unwrap(),
            Some(session) => {
                if let Err(refused) = driver.resume_session(&instance, &session, &split.session).await {
                    driver.stop_instance(&instance, Duration::from_secs(4)).await;
                    return Err(refused);
                }
                session
            }
        };
        let prompt = if id == "i2" { 2 } else { 1 };
        let key = PromptKey::new("r1", 1, prompt);
        driver
            .send_prompt(&instance, &session, "Do the task.", key)
            .await
            .unwrap();
        Ok(Self {
            provider,
            driver,
            instance,
            session,
            events,
            seen: Vec::new(),
            rig,
        })
    }

    /// Events until `until` holds for one, or `wait` passes; the matching event, if any.
    pub(crate) async fn until(&mut self, wait: Duration, until: impl Fn(&Normalized) -> bool) -> Option<Normalized> {
        let deadline = tokio::time::Instant::now() + wait;
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, self.events.recv()).await {
            self.seen.push(event.event.clone());
            if until(&event.event) {
                return Some(event.event);
            }
        }
        None
    }

    pub(crate) async fn ended(&mut self) -> StopReason {
        match self
            .until(Duration::from_secs(60), |e| matches!(e, Normalized::TurnEnded { .. }))
            .await
        {
            Some(Normalized::TurnEnded { stop_reason }) => stop_reason,
            _ => panic!("no turn end in {:?}", self.seen),
        }
    }

    pub(crate) fn failures(&self) -> Vec<&Failure> {
        self.seen
            .iter()
            .filter_map(|e| match e {
                Normalized::Failure(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    /// The first failure of a failed turn.
    pub(crate) async fn failed(&mut self) -> Failure {
        assert_eq!(self.ended().await, StopReason::Failed, "{:?}", self.seen);
        let failures = self.failures();
        assert!(!failures.is_empty(), "{:?}", self.seen);
        failures[0].clone()
    }

    pub(crate) async fn stop(mut self) {
        self.driver.stop_instance(&self.instance, Duration::from_secs(4)).await;
        self.until(Duration::from_secs(10), |e| matches!(e, Normalized::Exited { .. }))
            .await;
    }
}

pub(crate) async fn failure_of(reply: Reply, retries: u32) -> (Failure, Vec<Normalized>) {
    let dir = tempfile::tempdir().unwrap();
    let mut live = Live::start(vec![reply], dir.path(), retries, "never").await;
    let failure = live.failed().await;
    let seen = live.seen.clone();
    live.stop().await;
    (failure, seen)
}
