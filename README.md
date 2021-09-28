![Maintenance](https://img.shields.io/badge/maintenance-activly--developed-brightgreen.svg)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Released API docs](https://docs.rs/tokio-context/badge.svg)](https://docs.rs/tokio-context)

# tokio-context

## tokio-context

Provides a Golang like Context functionality. A Context in this respect is an object that is passed
around, primarily to async functions, that is used to determine if long running asynchronous
tasks should continue to run, or terminate.

You build a new Context by calling its `new()` constructor, which optionally takes a duration
that the context should run for. Calling the `new()` constructor returns the new `Context`
along with a `Handle`. The `Handle` can either have its `cancel()` method called,
or it can simply be dropped to cancel the context.

Please note that dropping the `Handle` **will** cancel the context.

If the duration supplied during Context construction elapses, then the Context will also be cancelled.

Here's one example of how you could use a passed in Context.

```rust
use tokio::time;
use tokio_context::Context;
use std::time::Duration;

async fn task_that_takes_too_long() {
    time::sleep(time::Duration::from_secs(60)).await;
    println!("done");
}

#[tokio::main]
async fn main() {
    // We've decided that we want a long running asynchronous task to last for a maximum of 1
    // second.
    let (mut ctx, _handle) = Context::new(Some(Duration::from_secs(1)));

    tokio::select! {
        _ = ctx.done() => return,
        _ = task_that_takes_too_long() => panic!("should never have gotten here"),
    }
}

```

While this may look no different than simply using tokio::time::timeout, we have retained a
handle that we can use to explicitely cancel the context, and any additionally spawned
contexts.


```rust
use std::time::Duration;
use tokio::time;
use tokio::task;
use tokio_context::Context;

async fn task_that_takes_too_long(mut ctx: Context) {
    tokio::select! {
        _ = ctx.done() => println!("cancelled early due to context"),
        _ = time::sleep(time::Duration::from_secs(60)) => println!("done"),
    }
}

#[tokio::main]
async fn main() {
    let (_, mut handle) = Context::new(None);

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

License: MIT
