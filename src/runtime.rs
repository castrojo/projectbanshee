//! Shared Tokio runtime for network/subprocess work. GTK code awaits the returned join
//! handles from `glib::spawn_future_local`, so results land back on the main loop.

use std::sync::LazyLock;
use tokio::runtime::Runtime;

static RT: LazyLock<Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_name("banshee-io")
        .enable_all()
        .build()
        .expect("failed to start the Tokio runtime")
});

pub fn runtime() -> &'static Runtime {
    &RT
}

/// Run `fut` on the Tokio runtime; await the result from any executor. A panic inside the
/// task is converted into an error string instead of unwinding into the caller.
pub async fn run<F, T>(fut: F) -> Result<T, String>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    RT.spawn(fut).await.map_err(|e| {
        if e.is_panic() {
            "internal error: background task panicked".to_string()
        } else {
            e.to_string()
        }
    })
}
