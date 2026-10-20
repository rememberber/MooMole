//! One bounded drain shared by ExitRequested and Cocoa's direct Exit path.
//! Callers may exit after TimedOut, but it never means native work is complete.
use crate::{engine::Job, updates};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

type Jobs = Mutex<HashMap<String, Arc<Job>>>;
const GRACE: Duration = Duration::from_secs(6);
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Request {
    Exit,
    Drain,
    Waiting,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Outcome {
    Waiting,
    Drained,
    TimedOut,
}
enum Phase {
    Accepting,
    Draining(Instant),
    Finished(Outcome),
}
pub(crate) struct Controller(Mutex<Phase>);
impl Default for Controller {
    fn default() -> Self {
        Self(Mutex::new(Phase::Accepting))
    }
}
impl Controller {
    pub fn closing(&self) -> bool {
        !matches!(
            *self.0.lock().unwrap_or_else(|e| e.into_inner()),
            Phase::Accepting
        )
    }
    pub fn request(&self, jobs: &Jobs, updates: &updates::Checker) -> Request {
        self.request_at(jobs, updates, Instant::now())
    }
    fn request_at(&self, jobs: &Jobs, updates: &updates::Checker, now: Instant) -> Request {
        // Admission and cancellation share the registry lock. Never hold it
        // while awaiting a worker, SQLite, or the native file operation.
        let jobs = jobs.lock().unwrap_or_else(|e| e.into_inner());
        let mut phase = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match *phase {
            Phase::Finished(_) => return Request::Exit,
            Phase::Draining(_) => return Request::Waiting,
            Phase::Accepting => {}
        }
        *phase = Phase::Draining(now + GRACE);
        updates.shutdown();
        for job in jobs.values() {
            job.cancel();
        }
        if jobs.is_empty() && !updates.active() {
            *phase = Phase::Finished(Outcome::Drained);
            Request::Exit
        } else {
            Request::Drain
        }
    }
    pub fn poll(&self, jobs: &Jobs, updates: &updates::Checker) -> Outcome {
        self.poll_at(jobs, updates, Instant::now())
    }
    fn poll_at(&self, jobs: &Jobs, updates: &updates::Checker, now: Instant) -> Outcome {
        let jobs = jobs.lock().unwrap_or_else(|e| e.into_inner());
        let mut phase = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let deadline = match *phase {
            Phase::Finished(outcome) => return outcome,
            Phase::Accepting => return Outcome::Waiting,
            Phase::Draining(deadline) => deadline,
        };
        if jobs.is_empty() && !updates.active() {
            *phase = Phase::Finished(Outcome::Drained);
            Outcome::Drained
        } else if now >= deadline {
            // Signal only currently owned child groups, once. An in-flight
            // Foundation operation has no process group and cannot be killed
            // or declared successful here. Leave its durable intent alone.
            for job in jobs.values() {
                job.kill_group();
            }
            *phase = Phase::Finished(Outcome::TimedOut);
            Outcome::TimedOut
        } else {
            Outcome::Waiting
        }
    }
}
#[cfg(test)]
mod tests;
