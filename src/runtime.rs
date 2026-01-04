use once_cell::sync::OnceCell;
use tokio::runtime::Runtime;

/// Global Tokio Runtime
static RUNTIME: OnceCell<Runtime> = OnceCell::new();

/// Get or create Runtime
pub fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime")
    })
}

/// Execute async code in Runtime context
/// 
/// Uses Runtime::block_on to ensure execution in the correct Tokio context
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    get_runtime().block_on(future)
}
