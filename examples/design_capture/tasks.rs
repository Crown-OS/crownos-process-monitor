//! Runs the window's background tasks on a tokio thread, as the real runner does, and hands
//! their answers back when the harness lets time pass.

use std::any::Any;
use std::sync::mpsc;

use anyhow::{Context, Result};
use view::{TaskExecutor, TaskFuture, TaskId, Ui};

type Finished = (TaskId, Box<dyn Any + Send>);

struct ThreadExecutor(tokio::sync::mpsc::UnboundedSender<(TaskId, TaskFuture)>);

impl TaskExecutor for ThreadExecutor {
    fn spawn(&mut self, id: TaskId, future: TaskFuture) {
        let _ = self.0.send((id, future));
    }

    fn cancel(&mut self, _id: TaskId) {}
}

/// The answers of tasks that finished, waiting for the UI thread.
pub struct TaskAnswers(mpsc::Receiver<Finished>);

impl TaskAnswers {
    /// Installs a task executor on `ui`; its thread stops with the window.
    pub fn install(ui: &mut Ui) -> Result<Self> {
        let (start, mut started) = tokio::sync::mpsc::unbounded_channel::<(TaskId, TaskFuture)>();
        let (finish, finished) = mpsc::channel();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("cannot start the task runtime")?;
        std::thread::spawn(move || {
            runtime.block_on(async move {
                while let Some((id, future)) = started.recv().await {
                    let finish = finish.clone();
                    tokio::spawn(async move {
                        let _ = finish.send((id, future.await));
                    });
                }
            });
        });
        ui.set_task_executor(Box::new(ThreadExecutor(start)));
        Ok(Self(finished))
    }

    pub fn deliver(&self, ui: &mut Ui) {
        while let Ok((id, output)) = self.0.try_recv() {
            ui.complete_task(id, output);
        }
    }
}
