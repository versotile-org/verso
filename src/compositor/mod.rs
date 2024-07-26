#![deny(unsafe_code)]

use std::rc::Rc;

use compositing_traits::{CompositorProxy, CompositorReceiver, ConstellationMsg};
use crossbeam_channel::Sender;
use profile_traits::{mem, time};
use servo::webrender_traits::RenderingContext;
use webrender::{api::DocumentId, RenderApi};

pub use compositor::{IOCompositor, MouseWindowEvent, ShutdownState};

mod compositor;
mod touch;

/// Data used to construct a compositor.
pub struct InitialCompositorState {
    /// A channel to the compositor.
    pub sender: CompositorProxy,
    /// A port on which messages inbound to the compositor can be received.
    pub receiver: CompositorReceiver,
    /// A channel to the constellation.
    pub constellation_chan: Sender<ConstellationMsg>,
    /// A channel to the time profiler thread.
    pub time_profiler_chan: time::ProfilerChan,
    /// A channel to the memory profiler thread.
    pub mem_profiler_chan: mem::ProfilerChan,
    /// Instance of webrender API
    pub webrender: webrender::Renderer,
    pub webrender_document: DocumentId,
    pub webrender_api: RenderApi,
    pub rendering_context: RenderingContext,
    pub webrender_gl: Rc<dyn servo::gl::Gl>,
    pub webxr_main_thread: webxr::MainThreadRegistry,
}
