use std::time::Duration;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::{sync::broadcast, time::Instant};

/// If a timeout duration is supplied, the Context will
/// return `true` from the `done()` method if called after the elapsed timeout. Otherwise it
/// will only return `true` from `done()` if it receives the command to `cancel()`.
pub struct Context {
    pub(crate) timeout: Option<Instant>,
    pub(crate) cancel_receiver: Receiver<()>,
}

/// A handle returned from constructing a new `Context`. Used to cancel the context. You can
/// explicitely call `cancel()`, or, you can simply drop the ContextHandle to cancel the context.
///
/// It's also used to spawn new contexts. This fits cleaner into Rusts ownership system. We can
/// only create new receivers by asking for them from the underlying Sender. This also ensures that
/// only the owner of the context handle can generate more contexts for its chidlren.
pub struct Handle {
    /// Copied over from context to allow us to spawn new contexts off the handle. This should be
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
    /// channel sending a final `None` signal down the channel, causing `done()` to immediately
    /// return.
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
    /// either the cancel signal is cancelled, or when the optional timeout has elapsed.
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
    /// calling cancel() on the CancelHandle that was returned during initial construction.
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
