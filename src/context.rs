/// # tokio-context
///
/// Provides two different methods for cancelling futures with a provided handle for cancelling all
/// related futures, with a fallback timeout mechanism. This is accomplished either with the
/// `Context` API, or with the `TaskController` API depending on a users needs.
///
/// ## Context
///
/// Provides Golang like `Context` functionality. A Context in this respect is an object that is passed
/// around, primarily to async functions, that is used to determine if long running asynchronous
/// tasks should continue to run, or terminate.
///
/// You build a new Context by calling its `new()` constructor, which optionally takes a duration
/// that the context should run for. Calling the `new()` constructor returns the new `Context`
/// along with a `Handle`. The `Handle` can either have its `cancel()` method called,
/// or it can simply be dropped to cancel the context.
///
/// Please note that dropping the `Handle` **will** cancel the context.
///
/// If the duration supplied during Context construction elapses, then the Context will also be cancelled.
///
/// # Examples
///
/// ```rust
/// use tokio::time;
/// use tokio_context::context::Context;
/// use std::time::Duration;
///
/// async fn task_that_takes_too_long() {
///     time::sleep(time::Duration::from_secs(60)).await;
///     println!("done");
/// }
///
/// #[tokio::main]
/// async fn main() {
///     // We've decided that we want a long running asynchronous task to last for a maximum of 1
///     // second.
///     let (mut ctx, _handle) = Context::new(Some(Duration::from_secs(1)));
///     
///     tokio::select! {
///         _ = ctx.done() => return,
///         _ = task_that_takes_too_long() => panic!("should never have gotten here"),
///     }
/// }
///
/// ```
///
/// While this may look no different than simply using tokio::time::timeout, we have retained a
/// handle that we can use to explicitly cancel the context, and any additionally spawned
/// contexts.
///
///
/// ```rust
/// use std::time::Duration;
/// use tokio::time;
/// use tokio::task;
/// use tokio_context::context::Context;
///
/// async fn task_that_takes_too_long(mut ctx: Context) {
///     tokio::select! {
///         _ = ctx.done() => println!("cancelled early due to context"),
///         _ = time::sleep(time::Duration::from_secs(60)) => println!("done"),
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let (_, mut handle) = Context::new(None);
///
///     let mut join_handles = vec![];
///
///     for i in 0..10 {
///         let mut ctx = handle.spawn_ctx();
///         let handle = task::spawn(async { task_that_takes_too_long(ctx).await });
///         join_handles.push(handle);
///     }
///
///     // Will cancel all spawned contexts.
///     handle.cancel();
///
///     // Now all join handles should gracefully close.
///     for join in join_handles {
///         join.await.unwrap();
///     }
/// }
///
/// ```
///
/// This pattern is useful if your child future needs to know about the cancel signal. This is
/// highly useful in many situations where a child future needs to perform graceful termination.
///
/// In instances where graceful termination of child futures is not needed, the API provided by
/// `tokio_context::task::TaskController` is much nicer to use. It doesn't pollute children with an
/// extra function argument of the context. It will however perform abrupt future termination,
/// which may not always be desired.
///
/// ## TaskController
///
/// Handles spawning tasks with a default timeout, which can also be cancelled by
/// calling cancel() on the task controller. If a Duration is supplied during construction of the
/// TaskController, then any tasks spawned by the TaskController will automatically be cancelled
/// after the supplied duration has elapsed.
///
/// This provides a different API from Context for the same end result. It's nicer to use when you
/// don't need child futures to gracefully shutdown. In cases that you do require graceful shutdown
/// of child futures, you will need to pass a Context down, and encorporate the context into normal
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
///     let mut controller = TaskController::new(None);
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
use std::time::Duration;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::{sync::broadcast, time::Instant};

/// An object that is passed around to asynchronous functions that may be used to check if the
/// function it was passed into should perform a graceful termination.
pub struct Context {
    pub(crate) timeout: Option<Instant>,
    pub(crate) cancel_receiver: Receiver<()>,
}

/// A handle returned from constructing a new `Context`. Used to cancel the context. You can
/// explicitly call `cancel()`, or, you can simply drop the Handle to cancel the context.
///
/// It's also used to spawn new contexts. This fits cleaner into Rusts ownership system. We can
/// only create new receivers by asking for them from the underlying Sender. This also ensures that
/// only the owner of the context handle can generate more contexts for its chidlren.
pub struct Handle {
    /// Allows us to spawn new contexts off the handle. This should be
    /// fine to clone because Instant is an instant in time, calculated at context construction as
    /// duration after construction time. It's an instant in time to be compared against, so
    /// results of future contexts spawned off this handle should be the same.
    pub(crate) timeout: Option<Instant>,
    /// Used to send a cancel signal to all spawned contexts off this handle.
    pub(crate) cancel_sender: Sender<()>,
}

impl Handle {
    /// Cancels the Context, ensuring that done() returns immediately.
    ///
    /// We only need to drop the handle, which will drop the sender. Doing so will close the
    /// channel, causing `done()` to immediately return.
    pub fn cancel(self) {}

    /// Will spawn a context identical to the context that was created along with this Handle. Used
    /// to generate more receivers for receiving a cancel signal from the parent.
    pub fn spawn_ctx(&mut self) -> Context {
        Context {
            timeout: self.timeout.clone(),
            cancel_receiver: self.cancel_sender.subscribe(),
        }
    }
}

impl Context {
    /// Builds a new Context. The done() method returns a future that will complete when
    /// either the handle is cancelled, or when the optional timeout has elapsed.
    pub fn new(timeout: Option<Duration>) -> (Context, Handle) {
        let timeout = if let Some(t) = timeout {
            Some(Instant::now() + t)
        } else {
            None
        };
        let (tx, _) = broadcast::channel(1);
        let mut handle = Handle {
            timeout,
            cancel_sender: tx,
        };
        (handle.spawn_ctx(), handle)
    }

    /// Blocks until either the provided timeout elapses, or a cancel signal is received from
    /// calling cancel() on the Handle that was returned during initial construction.
    #[allow(unused_must_use)] // We expect a receive error that the channel was closed.
    pub async fn done(&mut self) {
        if let Some(instant) = self.timeout {
            tokio::select! {
                _ = tokio::time::sleep_until(instant) => return,
                _ = self.cancel_receiver.recv() => return,
            }
        } else {
            // We expect a receive error that the channel was closed.
            self.cancel_receiver.recv().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cancel_handle_cancels_context() {
        let (mut ctx, handle) = Context::new(None);

        handle.cancel();

        tokio::select! {
            _ = ctx.done() => assert!(true),
            _ = tokio::time::sleep(Duration::from_millis(1)) => assert!(false),
        }
    }

    #[tokio::test]
    async fn duration_cancels_context() {
        // Note that we can't simply drop the handle here or the context will be cancelled.
        let (mut ctx, _handle) = Context::new(Some(Duration::from_millis(10)));

        tokio::select! {
            _ = ctx.done() => assert!(true),
            _ = tokio::time::sleep(Duration::from_millis(15)) => assert!(false),
        }
    }
}
