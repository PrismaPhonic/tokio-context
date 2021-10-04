use std::{future::Future, time::Duration};
use tokio::sync::broadcast::Sender;
use tokio::{sync::broadcast, time::Instant};

/// Handles spawning tasks which can also be cancelled by calling `cancel` on the task controller.
/// If a [`std::time::Duration`] is supplied using the
/// [`with_timeout`](fn@TaskController::with_timeout) constructor, then any tasks spawned by the
/// TaskController will automatically be cancelled after the supplied duration has elapsed.
///
/// This provides a different API from Context for the same end result. It's nicer to use when you
/// don't need child futures to gracefully shutdown. In cases that you do require graceful shutdown
/// of child futures, you will need to pass a Context down, and incorporate the context into normal
/// program flow for the child function so that they can react to it as needed and perform custom
/// asynchronous cleanup logic.
///
/// # Examples
///
/// ```rust
/// use std::time::Duration;
/// use tokio::time;
/// use tokio_context::task::TaskController;
///
/// async fn task_that_takes_too_long() {
///     time::sleep(time::Duration::from_secs(60)).await;
///     println!("done");
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let mut controller = TaskController::new();
///
///     let mut join_handles = vec![];
///
///     for i in 0..10 {
///         let handle = controller.spawn(async { task_that_takes_too_long().await });
///         join_handles.push(handle);
///     }
///
///     // Will cancel all spawned contexts.
///     controller.cancel();
///
///     // Now all join handles should gracefully close.
///     for join in join_handles {
///         join.await.unwrap();
///     }
/// }
/// ```
pub struct TaskController {
    timeout: Option<Instant>,
    cancel_sender: Sender<()>,
}

impl TaskController {
    /// Call cancel() to cancel any tasks spawned by this TaskController. You can also simply drop
    /// the TaskController to achieve the same result.
    pub fn cancel(self) {}

    /// Constructs a new TaskController, which can be used to spawn tasks. Tasks spawned from the
    /// task controller will be cancelled if `cancel()` gets called.
    pub fn new() -> TaskController {
        let (tx, _) = broadcast::channel(1);
        TaskController {
            timeout: None,
            cancel_sender: tx,
        }
    }

    /// Constructs a new TaskController, which can be used to spawn tasks. Tasks spawned from the
    /// task controller will be cancelled if `cancel()` gets called. They will also be cancelled if
    /// a supplied timeout elapses.
    pub fn with_timeout(timeout: Duration) -> TaskController {
        let (tx, _) = broadcast::channel(1);
        TaskController {
            timeout: Some(Instant::now() + timeout),
            cancel_sender: tx,
        }
    }

    /// Spawns tasks using an identical API to tokio::task::spawn. Tasks spawned from this
    /// TaskController will obey the optional timeout that may have been supplied during
    /// construction of the TaskController. They will also be cancelled if `cancel()` is ever
    /// called. Returns a JoinHandle from the internally generated task.
    pub fn spawn<T>(&mut self, future: T) -> tokio::task::JoinHandle<Option<T::Output>>
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let mut rx = self.cancel_sender.subscribe();
        if let Some(instant) = self.timeout {
            tokio::task::spawn(async move {
                tokio::select! {
                    res = future => Some(res),
                    _ = rx.recv() => None,
                    _ = tokio::time::sleep_until(instant) => None,
                }
            })
        } else {
            tokio::task::spawn(async move {
                tokio::select! {
                    res = future => Some(res),
                    _ = rx.recv() => None,
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cancel_handle_cancels_task() {
        let mut controller = TaskController::new();
        let join = controller.spawn(async { tokio::time::sleep(Duration::from_secs(60)).await });
        controller.cancel();

        tokio::select! {
            _ = join => assert!(true),
            _ = tokio::time::sleep(Duration::from_millis(1)) => assert!(false),
        }
    }

    #[tokio::test]
    async fn duration_cancels_task() {
        let mut controller = TaskController::with_timeout(Duration::from_millis(10));
        let join = controller.spawn(async { tokio::time::sleep(Duration::from_secs(60)).await });

        tokio::select! {
            _ = join => assert!(true),
            _ = tokio::time::sleep(Duration::from_millis(15)) => assert!(false),
        }
    }
}
