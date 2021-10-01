use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{broadcast, Mutex};
use tokio::time::Instant;

/// An object that is passed around to asynchronous functions that may be used to check if the
/// function it was passed into should perform a graceful termination.
///
/// You build a new Context by calling its [`new`](fn@Context::new)
/// constructor, which returns the new [`Context`] along with a [`Handle`]. The [`Handle`] can
/// either have its [`cancel`](fn@Context::cancel) method called, or it can simply be dropped to
/// cancel the context.
///
/// Please note that dropping the [`Handle`] **will** cancel the context.
///
/// If you would like to create a Context that automatically cancels after a given duration has
/// passed, use the [`with_timeout`](fn@Context::with_timeout) constructor. Using this
/// constructor will still give you a handle that can be used to immediately cancel the context as
/// well.
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
///     let (mut ctx, _handle) = Context::with_timeout(Duration::from_secs(1));
///     
///     tokio::select! {
///         _ = ctx.done() => return,
///         _ = task_that_takes_too_long() => panic!("should never have gotten here"),
///     }
/// }
///
/// ```
///
/// While this may look no different than simply using [`tokio::time::timeout`], we have retained a
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
///     let (_, mut handle) = Context::new();
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
/// The Context pattern is useful if your child future needs to know about the cancel signal. This is
/// highly useful in many situations where a child future needs to perform graceful termination.
///
/// If you would like to use chainable contexts, see [`RefContext`].
///
/// In instances where graceful termination of child futures is not needed, the API provided by
/// [`TaskController`] is much nicer to use. It doesn't pollute children with
/// an extra function argument of the context. It will however perform abrupt future termination,
/// which may not always be desired.
///
/// [`TaskController`]: crate::task::TaskController
pub struct Context {
    /// An optional timeout. After the timeout has elapsed calling done() will return immediately,
    /// indicating the context has been cancelled.
    pub(crate) timeout: Option<Instant>,
    /// A receiver used to receive a cancel signal from the parent handle that spawned this
    /// Context.
    pub(crate) cancel_receiver: Receiver<()>,
    /// Optionally a parent context that if cancelled will cancel this child context. Contexts can
    /// be perpetually chained together this way.
    pub(crate) parent_ctx: Option<RefContext>,
}

/// A way to share mutable access to a context. This is useful for context chaining. [`RefContext`]
/// contains an identical API to [`Context`].
///
/// Chaining a context means that the context will be cancelled if a parent context is
/// cancelled. A [`RefContext`] is simple a wrapper type around an `Arc<Mutex<Context>>` with an
/// identical API to [`Context`]. Here are a few examples to demonstrate how chainable contexts
/// work:
///
/// ```rust
/// use std::time::Duration;
/// use tokio::time;
/// use tokio::task;
/// use tokio_context::context::RefContext;
///
/// #[tokio::test]
/// async fn cancelling_parent_ctx_cancels_child() {
///     // Note that we can't simply drop the handle here or the context will be cancelled.
///     let (parent_ctx, parent_handle) = RefContext::new();
///     let (mut ctx, _handle) = Context::with_parent(&parent_ctx, None);
///
///     parent_handle.cancel();
///
///     // Cancelling a parent will cancel the child context.
///     tokio::select! {
///         _ = ctx.done() => assert!(true),
///         _ = tokio::time::sleep(Duration::from_millis(15)) => assert!(false),
///     }
/// }
///
/// #[tokio::test]
/// async fn cancelling_child_ctx_doesnt_cancel_parent() {
///     // Note that we can't simply drop the handle here or the context will be cancelled.
///     let (mut parent_ctx, _parent_handle) = RefContext::new();
///     let (_ctx, handle) = Context::with_parent(&parent_ctx, None);
///
///     handle.cancel();
///
///     // Cancelling a child will not cancel the parent context.
///     tokio::select! {
///         _ = parent_ctx.done() => assert!(false),
///         _ = async {} => assert!(true),
///     }
/// }
///
/// #[tokio::test]
/// async fn parent_timeout_cancels_child() {
///     // Note that we can't simply drop the handle here or the context will be cancelled.
///     let (parent_ctx, _parent_handle) = RefContext::with_timeout(Duration::from_millis(5));
///     let (mut ctx, _handle) =
///         Context::with_parent(&parent_ctx, Some(Duration::from_millis(10)));
///
///     tokio::select! {
///         _ = ctx.done() => assert!(true),
///         _ = tokio::time::sleep(Duration::from_millis(7)) => assert!(false),
///     }
/// }
/// ```
#[derive(Clone)]
pub struct RefContext(Arc<Mutex<Context>>);

/// A handle returned from constructing a new [`Context`]. Used to cancel the context. You can
/// explicitly call `cancel`, or, you can simply drop the Handle to cancel the context.
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
    /// Optionally used to support chainable contexts. If a context was created with a parent
    /// context chained to it, then any contexts spawned off this handle will also be a chained
    /// context.
    pub(crate) parent_ctx: Option<RefContext>,
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
        if let Some(ref ctx) = self.parent_ctx {
            Context {
                timeout: self.timeout.clone(),
                cancel_receiver: self.cancel_sender.subscribe(),
                parent_ctx: Some(ctx.clone()),
            }
        } else {
            Context {
                timeout: self.timeout.clone(),
                cancel_receiver: self.cancel_sender.subscribe(),
                parent_ctx: None,
            }
        }
    }

    /// Will spawn a RefContext identical to the RefContext that was created along with this
    /// Handle. If a Context was created with this handle, it will spawn a RefContext version of
    /// the original Context.
    pub fn spawn_ref(&mut self) -> RefContext {
        RefContext::from(self.spawn_ctx())
    }
}

impl Context {
    /// Builds a new Context without a timeout. The `done` method returns a future that will
    /// complete when this context is cancelled.
    pub fn new() -> (Context, Handle) {
        let (tx, _) = broadcast::channel(1);
        let mut handle = Handle {
            timeout: None,
            cancel_sender: tx,
            parent_ctx: None,
        };
        (handle.spawn_ctx(), handle)
    }

    /// Builds a new Context. The `done` method returns a future that will complete when
    /// either the handle is cancelled, or when the supplied timeout has elapsed.
    pub fn with_timeout(timeout: Duration) -> (Context, Handle) {
        let (tx, _) = broadcast::channel(1);
        let mut handle = Handle {
            timeout: Some(Instant::now() + timeout),
            cancel_sender: tx,
            parent_ctx: None,
        };
        (handle.spawn_ctx(), handle)
    }

    /// Builds a new Context, chained to a parent context. When `done` is called off a chained
    /// context it will return a future that will complete when either the handle is cancelled, the
    /// optional timeout has elapsed, the parent context is cancelled, or the optional parent timeout
    /// has elapsed.
    ///
    /// Note that using this version means that the context chain will end here. If you want to
    /// allow continuing the context chain, use [`RefContext::with_parent`].
    pub fn with_parent(parent_ctx: &RefContext, timeout: Option<Duration>) -> (Context, Handle) {
        let timeout = if let Some(t) = timeout {
            Some(Instant::now() + t)
        } else {
            None
        };
        let (tx, _) = broadcast::channel(1);
        let mut handle = Handle {
            timeout,
            cancel_sender: tx,
            parent_ctx: Some(parent_ctx.clone()),
        };
        (handle.spawn_ctx(), handle)
    }

    /// Blocks until either the provided timeout elapses, or a cancel signal is received from
    /// calling cancel() on the Handle that was returned during initial construction.
    #[allow(unused_must_use)] // We expect a receive error that the channel was closed.
    pub fn done(&mut self) -> Pin<Box<dyn Future<Output = ()> + '_ + Send>> {
        Box::pin(async move {
            match (self.timeout, self.parent_ctx.as_ref()) {
                // Non-chained cases.
                (Some(instant), None) => {
                    tokio::select! {
                        _ = tokio::time::sleep_until(instant) => return,
                        _ = self.cancel_receiver.recv() => return,
                    }
                }
                (None, None) => {
                    // We expect a receive error that the channel was closed.
                    self.cancel_receiver.recv().await;
                }
                // Chained cases.
                (Some(instant), Some(ctx)) => {
                    let parent_ctx = ctx.clone();
                    let mut inner = parent_ctx.0.lock().await;

                    tokio::select! {
                        _ = tokio::time::sleep_until(instant) => return,
                        _ = self.cancel_receiver.recv() => return,
                        _ = inner.done() => return,
                    }
                }
                (None, Some(ctx)) => {
                    let parent_ctx = ctx.clone();
                    let mut inner = parent_ctx.0.lock().await;

                    tokio::select! {
                        _ = self.cancel_receiver.recv() => return,
                        _ = inner.done() => return,
                    }
                }
            }
        })
    }
}

impl RefContext {
    /// Builds a new RefContext. The `done` method returns a future that will complete when
    /// either the handle is cancelled, or when the optional timeout has elapsed.
    pub fn new() -> (RefContext, Handle) {
        let (context, handle) = Context::new();
        (RefContext::from(context), handle)
    }

    /// Builds a new Context. The `done` method returns a future that will complete when
    /// either the handle is cancelled, or when the supplied timeout has elapsed.
    pub fn with_timeout(timeout: Duration) -> (RefContext, Handle) {
        let (context, handle) = Context::with_timeout(timeout);
        (RefContext::from(context), handle)
    }

    /// Builds a new RefContext, chained to a parent context. When `done` is called off a chained
    /// context it will return a future that will complete when either the handle is cancelled, the
    /// optional timeout has elapsed, the parent context is cancelled, or the optional parent timeout
    /// has elapsed.
    pub fn with_parent(parent_ctx: &RefContext, timeout: Option<Duration>) -> (RefContext, Handle) {
        let (context, handle) = Context::with_parent(parent_ctx, timeout);
        (RefContext::from(context), handle)
    }

    /// Blocks until either the provided timeout elapses, or a cancel signal is received from
    /// calling `cancel` on the Handle that was returned during initial construction.
    pub fn done(&mut self) -> Pin<Box<dyn Future<Output = ()> + '_>> {
        let soft_copy = self.clone();
        Box::pin(async move {
            let mut inner = soft_copy.0.lock().await;
            inner.done().await
        })
    }
}

impl From<Context> for RefContext {
    fn from(ctx: Context) -> Self {
        RefContext(Arc::new(Mutex::new(ctx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cancel_handle_cancels_context() {
        let (mut ctx, handle) = Context::new();

        handle.cancel();

        tokio::select! {
            _ = ctx.done() => assert!(true),
            _ = tokio::time::sleep(Duration::from_millis(1)) => assert!(false),
        }
    }

    #[tokio::test]
    async fn duration_cancels_context() {
        // Note that we can't simply drop the handle here or the context will be cancelled.
        let (mut ctx, _handle) = Context::with_timeout(Duration::from_millis(10));

        tokio::select! {
            _ = ctx.done() => assert!(true),
            _ = tokio::time::sleep(Duration::from_millis(15)) => assert!(false),
        }
    }

    #[tokio::test]
    async fn cancelling_parent_ctx_cancels_child() {
        // Note that we can't simply drop the handle here or the context will be cancelled.
        let (parent_ctx, parent_handle) = RefContext::new();
        let (mut ctx, _handle) = Context::with_parent(&parent_ctx, None);

        parent_handle.cancel();

        // Cancelling a parent will cancel the child context.
        tokio::select! {
            _ = ctx.done() => assert!(true),
            _ = tokio::time::sleep(Duration::from_millis(15)) => assert!(false),
        }
    }

    #[tokio::test]
    async fn cancelling_child_ctx_doesnt_cancel_parent() {
        // Note that we can't simply drop the handle here or the context will be cancelled.
        let (mut parent_ctx, _parent_handle) = RefContext::new();
        let (_ctx, handle) = Context::with_parent(&parent_ctx, None);

        handle.cancel();

        // Cancelling a child will not cancel the parent context.
        tokio::select! {
            _ = parent_ctx.done() => assert!(false),
            _ = async {} => assert!(true),
        }
    }

    #[tokio::test]
    async fn parent_timeout_cancels_child() {
        // Note that we can't simply drop the handle here or the context will be cancelled.
        let (parent_ctx, _parent_handle) = RefContext::with_timeout(Duration::from_millis(5));
        let (mut ctx, _handle) = Context::with_parent(&parent_ctx, Some(Duration::from_millis(10)));

        tokio::select! {
            _ = ctx.done() => assert!(true),
            _ = tokio::time::sleep(Duration::from_millis(7)) => assert!(false),
        }
    }
}
