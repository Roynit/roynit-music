use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Instant,
};
thread_local! {static CANCEL:RefCell<Option<Arc<AtomicBool>>>=const{RefCell::new(None)};}
pub fn check_cancelled() -> Result<()> {
    ensure!(
        !CANCEL.with(|c| c
            .borrow()
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))),
        "Task cancelled"
    );
    Ok(())
}
pub fn set_thread_cancel_token(token: Arc<AtomicBool>) {
    CANCEL.with(|c| *c.borrow_mut() = Some(token));
}

pub struct Completed {
    pub id: String,
    pub operation: String,
    pub target: Option<String>,
    pub result: Result<Value>,
    started: bool,
}
struct Queued {
    id: String,
    operation: String,
    target: Option<String>,
    task: Box<dyn FnOnce() -> Result<Value> + Send>,
    cancel: Arc<AtomicBool>,
}
pub struct Jobs {
    sender: mpsc::Sender<Completed>,
    receiver: mpsc::Receiver<Completed>,
    status: BTreeMap<String, Value>,
    running: usize,
    controls: BTreeMap<String, (Arc<AtomicBool>, Instant)>,
    queue: std::collections::VecDeque<Queued>,
}
impl Default for Jobs {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            status: BTreeMap::new(),
            running: 0,
            controls: BTreeMap::new(),
            queue: Default::default(),
        }
    }
}
impl Jobs {
    pub fn room(&self) -> usize {
        4096usize.saturating_sub(self.queue.len())
    }
    pub fn submit(
        &mut self,
        operation: String,
        target: Option<String>,
        task: impl FnOnce() -> Result<Value> + Send + 'static,
    ) -> Result<Value> {
        ensure!(
            self.room() > 0,
            "Task queue is full; wait for current tasks to finish"
        );
        if self.status.len() >= 100
            && let Some(id) = self
                .status
                .iter()
                .find(|(_, v)| v["state"] != "running" && v["state"] != "queued")
                .map(|(k, _)| k.clone())
        {
            self.status.remove(&id);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        self.status.insert(
            id.clone(),
            json!({"id":id,"state":"queued","operation":operation}),
        );
        self.controls
            .insert(id.clone(), (cancel.clone(), Instant::now()));
        self.queue.push_back(Queued {
            id: id.clone(),
            operation,
            target,
            task: Box::new(task),
            cancel,
        });
        self.launch();
        Ok(json!({"job_id":id,"state":self.status[&id]["state"]}))
    }
    fn launch(&mut self) {
        while let Some(index) = self
            .queue
            .iter()
            .position(|q| q.cancel.load(Ordering::Relaxed))
        {
            let q = self.queue.remove(index).expect("queued cancellation");
            let _ = self.sender.send(Completed {
                id: q.id,
                operation: q.operation,
                target: q.target,
                result: Err(anyhow::anyhow!("Task cancelled")),
                started: false,
            });
        }
        while self.running < 8 {
            let active = self
                .status
                .iter()
                .filter(|(id, v)| {
                    v["state"] == "running"
                        && self
                            .controls
                            .get(*id)
                            .is_some_and(|(c, _)| !c.load(Ordering::Relaxed))
                })
                .count();
            let priority = if active < 6 {
                self.queue
                    .iter()
                    .position(|q| q.operation == "playback.prepare")
            } else {
                None
            };
            let interactive = if active < 5 {
                self.queue.iter().position(|q| {
                    ["search", "music.search", "spotify.search"].contains(&q.operation.as_str())
                })
            } else {
                None
            };
            let index = priority
                .or(interactive)
                .or_else(|| (active < 4 && !self.queue.is_empty()).then_some(0));
            let Some(index) = index else {
                break;
            };
            let q = self.queue.remove(index).expect("queued task");
            self.running += 1;
            self.status.get_mut(&q.id).expect("task status")["state"] = json!("running");
            let sender = self.sender.clone();
            std::thread::spawn(move || {
                CANCEL.with(|c| *c.borrow_mut() = Some(q.cancel.clone()));
                let mut result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(q.task))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("Background worker panicked")));
                if q.cancel.load(Ordering::Relaxed) {
                    result = Err(anyhow::anyhow!("Task cancelled"));
                }
                CANCEL.with(|c| c.borrow_mut().take());
                let _ = sender.send(Completed {
                    id: q.id,
                    operation: q.operation,
                    target: q.target,
                    result,
                    started: true,
                });
            });
        }
    }
    pub fn drain(&mut self) -> Vec<Completed> {
        let jobs: Vec<_> = self
            .receiver
            .try_iter()
            .map(|mut q| {
                if self
                    .controls
                    .get(&q.id)
                    .is_some_and(|(c, _)| c.load(Ordering::Relaxed))
                {
                    q.result = Err(anyhow::anyhow!("Task cancelled"));
                }
                q
            })
            .collect();
        self.running = self
            .running
            .saturating_sub(jobs.iter().filter(|q| q.started).count());
        jobs
    }
    pub fn complete(&mut self, job: &Completed) {
        let cancelled = self
            .controls
            .remove(&job.id)
            .is_some_and(|(c, _)| c.load(Ordering::Relaxed));
        let value = match &job.result {
            Ok(v) => json!({"id":job.id,"state":"completed","operation":job.operation,"data":v}),
            Err(e) => {
                json!({"id":job.id,"state":if cancelled{"cancelled"}else{"failed"},"operation":job.operation,"error":format!("{e:#}")})
            }
        };
        self.status.insert(job.id.clone(), value);
        self.launch();
    }
    pub fn get(&self, id: &str) -> Result<Value> {
        let mut value = self
            .status
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Unknown job"))?;
        if let Some((cancel, start)) = self.controls.get(id) {
            value["elapsed_ms"] = json!(start.elapsed().as_millis());
            value["cancel_requested"] = json!(cancel.load(Ordering::Relaxed));
        }
        Ok(value)
    }
    pub fn activity(&self) -> Value {
        json!(self.controls.iter().map(|(id,(_,start))|json!({"id":id,"state":self.status[id]["state"],"operation":self.status[id]["operation"],"elapsed_ms":start.elapsed().as_millis()})).collect::<Vec<_>>())
    }
    pub fn list(&self) -> Value {
        json!(self.status.values().collect::<Vec<_>>())
    }
    pub fn cancel(&mut self, id: &str) -> Result<Value> {
        self.controls
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Job is already finished or unknown"))?
            .0
            .store(true, Ordering::Relaxed);
        self.launch();
        self.get(id)
    }
    pub fn cancel_operations(&mut self, operations: &[&str]) -> usize {
        let ids: Vec<_> = self
            .controls
            .keys()
            .filter(|id| {
                self.status
                    .get(*id)
                    .and_then(|v| v["operation"].as_str())
                    .is_some_and(|op| operations.contains(&op))
            })
            .cloned()
            .collect();
        for id in &ids {
            if let Some((cancel, _)) = self.controls.get(id) {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        self.launch();
        ids.len()
    }
    pub fn cancel_all(&mut self) -> Value {
        let count = self.controls.len();
        for (cancel, _) in self.controls.values() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.launch();
        json!({"cancel_requested":count})
    }
}
impl Drop for Jobs {
    fn drop(&mut self) {
        for (cancel, _) in self.controls.values() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.queue.clear();
        let until = Instant::now() + std::time::Duration::from_millis(400);
        while self.running > 0 && Instant::now() < until {
            match self
                .receiver
                .recv_timeout(std::time::Duration::from_millis(30))
            {
                Ok(q) => {
                    if q.started {
                        self.running = self.running.saturating_sub(1);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playback_has_reserved_capacity_and_queued_cancellation_never_executes() {
        let mut jobs = Jobs::default();
        let gate = Arc::new(AtomicBool::new(false));
        for _ in 0..4 {
            let gate = gate.clone();
            jobs.submit("download".into(), None, move || {
                while !gate.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Ok(json!(true))
            })
            .unwrap();
        }
        let executed = Arc::new(AtomicBool::new(false));
        let flag = executed.clone();
        let queued = jobs
            .submit("download".into(), None, move || {
                flag.store(true, Ordering::Relaxed);
                Ok(json!(true))
            })
            .unwrap();
        let search = jobs
            .submit("music.search".into(), None, || Ok(json!("results")))
            .unwrap();
        let playback = jobs
            .submit("playback.prepare".into(), None, || Ok(json!("ready")))
            .unwrap();
        let queued_before = jobs.get(queued["job_id"].as_str().unwrap()).unwrap()["state"].clone();
        jobs.cancel(queued["job_id"].as_str().unwrap()).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        let mut played = false;
        let mut searched = false;
        let mut cancelled = false;
        while Instant::now() < deadline && !(played && searched && cancelled) {
            for done in jobs.drain() {
                if done.id == search["job_id"].as_str().unwrap() {
                    searched = done.result.is_ok();
                }
                if done.id == playback["job_id"].as_str().unwrap() {
                    played = done.result.is_ok();
                }
                if done.id == queued["job_id"].as_str().unwrap() {
                    cancelled = done.result.is_err();
                }
                jobs.complete(&done);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        gate.store(true, Ordering::Relaxed);
        assert_eq!(queued_before, "queued");
        assert!(played, "Playback must not wait for occupied download slots");
        assert!(cancelled);
        assert!(searched, "Search must not wait for occupied download slots");
        assert!(!executed.load(Ordering::Relaxed));
    }
}
