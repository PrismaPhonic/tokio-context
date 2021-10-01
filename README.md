![Maintenance](https://img.shields.io/badge/maintenance-activly--developed-brightgreen.svg)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Released API docs](https://docs.rs/tokio-context/badge.svg)](https://docs.rs/tokio-context)

# tokio-context

Provides two different methods for cancelling futures with a provided handle for cancelling all
related futures, with a fallback timeout mechanism. This is accomplished either with the
`Context` API, or with the `TaskController` API depending on a users needs.

### Context

Provides Golang like Context functionality. A Context in this respect is an object that is
passed around, primarily to async functions, that is used to determine if long running
asynchronous tasks should continue to run, or terminate.

You build a new Context by calling its `new` constructor, which returns the new
`Context` along with a `Handle`. The `Handle` can either have its `cancel`
method called, or it can simply be dropped to cancel the context.

Please note that dropping the `Handle` **will** cancel the context.

If you would like to create a Context that automatically cancels after a given duration has
passed, use the `with_timeout` constructor. Using this constructor will still
give you a handle that can be used to immediately cancel the context as well.

## Examples

```rust
use tokio::time;
use tokio_context::context::Context;
use std::time::Duration;

async fn task_that_takes_too_long() {
    time::sleep(time::Duration::from_secs(60)).await;
    println!("done");
}

#[tokio::main]
async fn main() {
    // We've decided that we want a long running asynchronous task to last for a maximum of 1
    // second.
    let (mut ctx, _handle) = Context::with_timeout(Duration::from_secs(1));

    tokio::select! {
        _ = ctx.done() => return,
        _ = task_that_takes_too_long() => panic!("should never have gotten here"),
    }
}

```

While this may look no different than simply using `tokio::time::timeout`, we have retained a
handle that we can use to explicitly cancel the context, and any additionally spawned
contexts.


```rust
use std::time::Duration;
use tokio::time;
use tokio::task;
use tokio_context::context::Context;

async fn task_that_takes_too_long(mut ctx: Context) {
    tokio::select! {
        _ = ctx.done() => println!("cancelled early due to context"),
        _ = time::sleep(time::Duration::from_secs(60)) => println!("done"),
    }
}

#[tokio::main]
async fn main() {
    let (_, mut handle) = Context::new();

    let mut join_handles = vec![];

    for i in 0..10 {
        let mut ctx = handle.spawn_ctx();
        let handle = task::spawn(async { task_that_takes_too_long(ctx).await });
        join_handles.push(handle);
    }

    // Will cancel all spawned contexts.
    handle.cancel();

    // Now all join handles should gracefully close.
    for join in join_handles {
        join.await.unwrap();
    }
}

```

Contexts may also be chained by using the `with_parent` constructor in conjunction with
RefContexts. Chaining a context means that the context will be cancelled if a parent context is
cancelled. A `RefContext` is simple a wrapper type around an `Arc<Mutex<Context>>` with an
identical API to `Context`. Here are a few examples to demonstrate how chainable contexts work:

```rust
use std::time::Duration;
use tokio::time;
use tokio::task;
use tokio_context::context::RefContext;

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
    let (mut ctx, _handle) =
        Context::with_parent(&parent_ctx, Some(Duration::from_millis(10)));

    tokio::select! {
        _ = ctx.done() => assert!(true),
        _ = tokio::time::sleep(Duration::from_millis(7)) => assert!(false),
    }
}
```

The Context pattern is useful if your child future needs to know about the cancel signal. This
is highly useful in many situations where a child future needs to perform graceful termination.

In instances where graceful termination of child futures is not needed, the API provided by
`TaskController` is much nicer to use. It doesn't pollute children with an extra
function argument of the context. It will however perform abrupt future
termination, which may not always be desired.

### TaskController

Handles spawning tasks which can also be cancelled by calling `cancel` on the
task controller. If a `std::time::Duration` is supplied using the `with_timeout`
constructor, then any tasks spawned by the TaskController will automatically be
cancelled after the supplied duration has elapsed.

This provides a different API from Context for the same end result. It's nicer
to use when you don't need child futures to gracefully shutdown. In cases that
you do require graceful shutdown of child futures, you will need to pass a
Context down, and incorporate the context into normal program flow for the child
function so that they can react to it as needed and perform custom asynchronous
cleanup logic.

## Examples

```rust
use std::time::Duration;
use tokio::time;
use tokio_context::task::TaskController;

async fn task_that_takes_too_long() {
    time::sleep(time::Duration::from_secs(60)).await;
    println!("done");
}

#[tokio::main]
async fn main() {
    let mut controller = TaskController::new();

    let mut join_handles = vec![];

    for i in 0..10 {
        let handle = controller.spawn(async { task_that_takes_too_long().await });
        join_handles.push(handle);
    }

    // Will cancel all spawned contexts.
    controller.cancel();

    // Now all join handles should gracefully close.
    for join in join_handles {
        join.await.unwrap();
    }
}
```

License: MIT
