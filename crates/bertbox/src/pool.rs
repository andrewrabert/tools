use std::num::NonZeroUsize;
use std::sync::{Mutex, PoisonError, mpsc};
use std::thread;

use anyhow::{Context, Result};

pub fn jobs(num_procs: usize) -> Result<NonZeroUsize> {
    match NonZeroUsize::new(num_procs) {
        Some(jobs) => Ok(jobs),
        None => thread::available_parallelism().context("counting cpus"),
    }
}

pub fn run<J, T>(
    jobs: Vec<J>,
    workers: NonZeroUsize,
    work: impl Fn(&J) -> T + Sync,
    mut completed: impl FnMut(J, T) -> Result<()>,
) -> Result<()>
where
    J: Send,
    T: Send,
{
    let queue = Mutex::new(jobs.into_iter());
    thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel();
        for _ in 0..workers.get() {
            let sender = sender.clone();
            let (queue, work) = (&queue, &work);
            scope.spawn(move || {
                loop {
                    let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                    let Some(job) = next else { break };
                    let result = work(&job);
                    if sender.send((job, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        for (job, result) in receiver {
            completed(job, result)?;
        }
        Ok(())
    })
}
