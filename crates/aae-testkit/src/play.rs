//! Playing scenarios: events into the instance's sink, the death of a process.

use super::*;

impl ScriptedDriver {
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The session behind `session` if it lives on `instance` and the instance is alive.
    pub(crate) fn live_session<'a>(
        state: &'a mut State,
        instance: &InstanceHandle,
        session: &SessionRef,
    ) -> Result<&'a mut Session, Failure> {
        if !state.instances.get(&instance.id).is_some_and(|i| i.alive) {
            return Err(Failure::new(FailureKind::HarnessCrash, Source::Adapter));
        }
        match state.sessions.get_mut(&session_id(session)?) {
            Some(s) if s.instance == instance.id => Ok(s),
            _ => Err(Failure::new(FailureKind::TranscriptDivergence, Source::Adapter)),
        }
    }

    /// Plays the next scenario on `session`, after `first` if given.
    pub(crate) fn play(
        &self,
        instance: &InstanceHandle,
        session: &SessionRef,
        first: Option<Normalized>,
    ) -> Result<(), Failure> {
        let mut state = self.lock();
        Self::live_session(&mut state, instance, session)?;
        let steps = state.scenarios.pop_front().unwrap_or_default();
        let first = first.filter(|_| !matches!(steps.first(), Some(Step::NoAck)));
        let target = Self::live_session(&mut state, instance, session)?;
        if let Some(previous) = target.playing.take() {
            previous.abort();
        }
        target.in_turn = true;
        let (driver, instance, session) = (self.clone(), instance.id.clone(), session.clone());
        target.playing = Some(tokio::spawn(async move {
            if let Some(event) = first {
                driver.emit(&instance, Some(&session), event);
            }
            for step in steps {
                match step {
                    Step::NoAck => {}
                    Step::Wait(d) => tokio::time::sleep(d).await,
                    Step::Await(signal) => signal.notified().await,
                    Step::Emit(event) => driver.emit(&instance, Some(&session), event),
                    Step::Exit { code, signal } => {
                        driver.exit(&instance, code, signal);
                        return;
                    }
                    Step::Vanish => {
                        if let Some(i) = driver.lock().instances.get_mut(&instance) {
                            i.alive = false;
                        }
                        return;
                    }
                }
            }
        }));
        Ok(())
    }

    pub(crate) fn emit(&self, instance: &str, session: Option<&SessionRef>, event: Normalized) {
        let mut state = self.lock();
        if let (Some(session), Normalized::TurnEnded { stop_reason }) = (session, &event)
            && let Some(last) = session_id(session).ok().and_then(|id| state.sessions.get_mut(&id))
            && let Some(prompt) = last.prompts.last_mut()
        {
            prompt.1 |= *stop_reason == StopReason::Completed;
            last.in_turn = false;
        }
        if let Some(i) = state.instances.get_mut(instance).filter(|i| i.alive) {
            i.seq += 1;
            let _ = i.sink.send(DriverEvent {
                instance: instance.to_owned(),
                session: session.cloned(),
                seq: i.seq,
                event,
            });
        }
    }

    pub(crate) fn exit(&self, instance: &str, code: Option<i32>, signal: Option<i32>) {
        self.emit(instance, None, Normalized::Exited { code, signal });
        let mut state = self.lock();
        if let Some(i) = state.instances.get_mut(instance) {
            i.alive = false;
        }
        for session in state.sessions.values_mut().filter(|s| s.instance == instance) {
            if let Some(playing) = session.playing.take() {
                playing.abort();
            }
        }
    }
}

pub(crate) fn session_id(session: &SessionRef) -> Result<u64, Failure> {
    session.0["session"]
        .as_u64()
        .ok_or_else(|| Failure::new(FailureKind::TranscriptDivergence, Source::Adapter))
}
