//! Getting hold of a device, and the two blocking helpers the headless path
//! needs. Everything above this module is platform-independent.

use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Mutex, OnceLock};
use std::task::{Context, Poll};

use crate::Error;

/// A device, its queue and the adapter they came from.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The most recent uncaptured device error, if any. wgpu reports
    /// validation errors through a callback rather than a `Result`, and a
    /// headless renderer has nowhere to log them, so they are collected here
    /// and turned into an `Err` from `render()`.
    pub errors: Arc<ErrorSink>,
}

#[derive(Default)]
pub struct ErrorSink {
    count: AtomicU32,
    message: std::sync::Mutex<Option<String>>,
}

impl ErrorSink {
    fn record(&self, e: wgpu::Error) {
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut slot = self.message.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(e.to_string());
        }
    }

    /// The first error since the last call, if any.
    pub fn take(&self) -> Option<String> {
        if self.count.swap(0, Ordering::Relaxed) == 0 {
            return None;
        }
        self.message.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// Serialises [`Gpu::open`] across the process. Native only: in a browser
/// there is one thread and no Vulkan loader, and a guard held across the
/// awaits there would only block a second, interleaved open.
#[cfg(not(target_arch = "wasm32"))]
static OPENING: Mutex<()> = Mutex::new(());

/// The process's one instance. `..._from_env` honours WGPU_BACKEND /
/// WGPU_ADAPTER_NAME, which is how a CI machine with several adapters pins
/// the one goldens were made on; it is read once, at the first open.
#[cfg(not(target_arch = "wasm32"))]
fn shared_instance() -> wgpu::Instance {
    static INSTANCE: OnceLock<wgpu::Instance> = OnceLock::new();
    INSTANCE
        .get_or_init(|| wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()))
        .clone()
}

/// On wasm32 wgpu's handles are not `Sync`, so there is no process-wide
/// instance; each open makes its own, as before.
#[cfg(target_arch = "wasm32")]
fn shared_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

/// How to pick an adapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdapterPreference {
    /// Whatever the platform thinks is best: a real GPU where there is one.
    #[default]
    Auto,
    /// Only a software rasterizer (lavapipe, SwiftShader). CI and goldens.
    Software,
}

/// Human-readable description of the adapter a renderer is running on, for
/// diagnostics and test output: `"llvmpipe (Vulkan, Cpu)"`.
pub fn describe(info: &wgpu::AdapterInfo) -> String {
    format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
}

impl Gpu {
    /// Open a device. `None` from `request_adapter` is the "no GPU here" case
    /// that CI must skip on, and is reported as [`Error::NoAdapter`].
    ///
    /// Every renderer in the process shares one `wgpu::Instance`, and opens
    /// are taken one at a time. The Vulkan loader is not safe against two
    /// instances being set up at once: enumerating physical devices on a new
    /// instance `dlclose`s the drivers that report none, and a second thread
    /// creating its device through the loader at that moment segfaulted
    /// (`renderers_open_concurrently`). One instance loads and prunes the
    /// drivers once; the lock keeps adapter and device requests apart.
    pub async fn open(pref: AdapterPreference) -> Result<Gpu, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        let _one_at_a_time = OPENING.lock().unwrap_or_else(|e| e.into_inner());
        let instance = shared_instance();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: pref == AdapterPreference::Software,
                compatible_surface: None,
            })
            .await
            .map_err(|e| Error::NoAdapter(e.to_string()))?;
        Gpu::from_adapter(instance, adapter).await
    }

    pub async fn from_adapter(instance: wgpu::Instance, adapter: wgpu::Adapter) -> Result<Gpu, Error> {
        // The limits are the WebGPU downlevel defaults so the same code runs in
        // a browser. The one optional feature: an adapter's own per-format
        // capabilities (e.g. 8x MSAA on Rgba16Float), taken when offered.
        // Without it a device allows only what WebGPU guarantees, whatever the
        // adapter reports; Renderer::sample_count asks the same question.
        let required_features = adapter.features() & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("cadpole"),
                required_features,
                required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|e| Error::Device(e.to_string()))?;
        let errors = Arc::new(ErrorSink::default());
        let sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e| sink.record(e)));
        Ok(Gpu { instance, adapter, device, queue, errors })
    }

    /// Run the queue dry and surface any validation error as an `Err`.
    pub fn flush(&self) -> Result<(), Error> {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        match self.errors.take() {
            Some(m) => Err(Error::Device(m)),
            None => Ok(()),
        }
    }
}

/// Drive a future to completion on this thread.
///
/// Every future wgpu hands back on a native backend is already resolved by the
/// time it is first polled (adapter and device requests are synchronous inside
/// wgpu-core) and buffer mapping is driven by [`Gpu::flush`] rather than by a
/// waker, so a parker-free loop is enough and saves a dependency on an async
/// runtime. Never used on wasm32, where the browser owns the event loop.
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = pin!(fut);
    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
