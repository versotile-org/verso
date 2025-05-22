use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base::cross_process_instant::CrossProcessInstant;
use base::id::{PipelineId, WebViewId};
use base::{Epoch, WebRenderEpochToU16};
use compositing_traits::display_list::{CompositorDisplayListInfo, HitTestInfo, ScrollTree};
use compositing_traits::{
    CompositionPipeline, CompositorMsg, CompositorProxy, ImageUpdate, SendableFrameTree,
};
use constellation_traits::{
    AnimationTickType, EmbedderToConstellationMessage, PaintMetricEvent, ScrollState,
    WindowSizeType,
};
use crossbeam_channel::{Receiver, Sender};
use dpi::PhysicalSize;
use embedder_traits::{
    AnimationState, CompositorHitTestResult, Cursor, InputEvent, MouseButton, MouseButtonAction,
    MouseButtonEvent, MouseMoveEvent, TouchEvent, TouchEventType, TouchId, UntrustedNodeAddress,
    ViewportDetails,
};
use euclid::{Point2D, Scale, Size2D, Transform3D, Vector2D, vec2};
use gleam::gl;
use ipc_channel::ipc::{self, IpcSharedMemory};
use log::{debug, error, trace, warn};
use profile_traits::mem::{ProcessReports, Report, ReportKind};
use profile_traits::time::{self as profile_time, ProfilerCategory};
use profile_traits::{mem, path, time, time_profile};
use servo_geometry::{DeviceIndependentIntSize, DeviceIndependentPixel};
use style_traits::CSSPixel;
use webrender::{RenderApi, Transaction};
use webrender_api::units::{
    DeviceIntPoint, DeviceIntRect, DevicePixel, DevicePoint, DeviceRect, DeviceSize, LayoutPoint,
    LayoutRect, LayoutSize, LayoutVector2D, WorldPoint,
};
use webrender_api::{
    BorderRadius, BoxShadowClipMode, BuiltDisplayList, ClipMode, ColorF, CommonItemProperties,
    ComplexClipRegion, DirtyRect, DisplayListPayload, DocumentId, Epoch as WebRenderEpoch,
    ExternalScrollId, FontInstanceFlags, FontInstanceKey, FontInstanceOptions, FontKey,
    HitTestFlags, PipelineId as WebRenderPipelineId, PropertyBinding, ReferenceFrameKind,
    RenderReasons, SampledScrollOffset, ScrollLocation, SpaceAndClipInfo, SpatialId,
    SpatialTreeItemKey, TransformStyle,
};
use winit::window::WindowId;

use crate::rendering::RenderingContext;
use crate::touch::{TouchAction, TouchHandler};
use crate::window::Window;

/// Data used to construct a compositor.
pub struct InitialCompositorState {
    /// A channel to the compositor.
    pub sender: CompositorProxy,
    /// A port on which messages inbound to the compositor can be received.
    pub receiver: Receiver<CompositorMsg>,
    /// A channel to the constellation.
    pub constellation_chan: Sender<EmbedderToConstellationMessage>,
    /// A channel to the time profiler thread.
    pub time_profiler_chan: time::ProfilerChan,
    /// A channel to the memory profiler thread.
    pub mem_profiler_chan: mem::ProfilerChan,
    /// Instance of webrender API
    pub webrender: webrender::Renderer,
    /// Webrender document ID
    pub webrender_document: DocumentId,
    /// Webrender API
    pub webrender_api: RenderApi,
    /// Servo's rendering context
    pub rendering_context: RenderingContext,
    /// Webrender GL handle
    pub webrender_gl: Rc<dyn gl::Gl>,
}

/// Various debug and profiling flags that WebRender supports.
#[derive(Clone)]
pub enum WebRenderDebugOption {
    /// Set profiler flags to webrender.
    Profiler,
    /// Set texture cache flags to webrender.
    TextureCacheDebug,
    /// Set render target flags to webrender.
    RenderTargetDebug,
}

/// Mouse event for the compositor.
#[derive(Clone)]
pub enum MouseWindowEvent {
    /// Mouse click event
    Click(MouseButton, DevicePoint),
    /// Mouse down event
    MouseDown(MouseButton, DevicePoint),
    /// Mouse up event
    MouseUp(MouseButton, DevicePoint),
}

// NB: Never block on the Constellation, because sometimes the Constellation blocks on us.
/// The Verso compositor contains a GL rendering context with a WebRender instance.
/// The compositor will communicate with Servo using messages from the Constellation,
/// then composite the WebRender frames and present the surface to the window.
pub struct IOCompositor {
    /// The current window that Compositor is handling.
    pub current_window: WindowId,

    /// Size of current viewport that Compositor is handling.
    viewport: DeviceSize,

    /// The pixel density of the display.
    scale_factor: Scale<f32, DeviceIndependentPixel, DevicePixel>,

    /// The active webrender document.
    webrender_document: DocumentId,

    /// The port on which we receive messages.
    compositor_receiver: Receiver<CompositorMsg>,

    /// Tracks each webview and its current pipeline
    webviews: HashMap<WebViewId, PipelineId>,

    /// Tracks details about each active pipeline that the compositor knows about.
    pipeline_details: HashMap<PipelineId, PipelineDetails>,

    /// Tracks whether we should composite this frame.
    composition_request: CompositionRequest,

    /// check if the surface is ready to present.
    pub ready_to_present: bool,

    /// Tracks whether we are in the process of shutting down, or have shut down and should close
    /// the compositor.
    pub shutdown_state: ShutdownState,

    /// The current frame tree ID (used to reject old paint buffers)
    frame_tree_id: FrameTreeId,

    /// The channel on which messages can be sent to the constellation.
    pub constellation_chan: Sender<EmbedderToConstellationMessage>,

    /// The channel on which messages can be sent to the time profiler.
    time_profiler_chan: profile_time::ProfilerChan,

    /// Touch input state machine
    touch_handler: TouchHandler,

    /// Pending scroll/zoom events.
    pending_scroll_zoom_events: Vec<ScrollZoomEvent>,

    /// Used by the logic that determines when it is safe to output an
    /// image for the reftest framework.
    ready_to_save_state: ReadyState,

    /// The webrender renderer.
    webrender: Option<webrender::Renderer>,

    /// The webrender interface, if enabled.
    pub webrender_api: RenderApi,

    /// The glutin instance that webrender targets
    pub rendering_context: RenderingContext,

    /// The GL bindings for webrender
    webrender_gl: Rc<dyn gl::Gl>,

    /// Current mouse cursor.
    cursor: Cursor,

    /// Current cursor position.
    cursor_pos: DevicePoint,

    /// True to exit after page load ('-x').
    wait_for_stable_image: bool,

    /// True to translate mouse input into touch events.
    convert_mouse_to_touch: bool,

    /// The number of frames pending to receive from WebRender.
    pending_frames: usize,

    /// The [`Instant`] of the last animation tick, used to avoid flooding the Constellation and
    /// ScriptThread with a deluge of animation ticks.
    last_animation_tick: Instant,

    /// Whether the application is currently animating.
    /// Typically, when animations are active, the window
    /// will want to avoid blocking on UI events, and just
    /// run the event loop at the vsync interval.
    pub is_animating: bool,
}

#[derive(Clone, Copy)]
struct ScrollEvent {
    /// Scroll by this offset, or to Start or End
    scroll_location: ScrollLocation,
    /// Apply changes to the frame at this location
    cursor: DeviceIntPoint,
    /// The number of OS events that have been coalesced together into this one event.
    event_count: u32,
}

#[derive(Clone, Copy)]
enum ScrollZoomEvent {
    /// An pinch zoom event that magnifies the view by the given factor.
    PinchZoom(f32),
    /// A scroll event that scrolls the scroll node at the given location by the
    /// given amount.
    Scroll(ScrollEvent),
}

/// Why we performed a composite. This is used for debugging.
///
/// TODO: It would be good to have a bit more precision here about why a composite
/// was originally triggered, but that would require tracking the reason when a
/// frame is queued in WebRender and then remembering when the frame is ready.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CompositingReason {
    /// We're performing the single composite in headless mode.
    Headless,
    /// We're performing a composite to run an animation.
    Animation,
    /// A new WebRender frame has arrived.
    NewWebRenderFrame,
    /// The window has been resized and will need to be synchronously repainted.
    Resize,
}

#[derive(Debug, PartialEq)]
enum CompositionRequest {
    NoCompositingNecessary,
    CompositeNow(CompositingReason),
}

/// Shutdown State of the compositor
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShutdownState {
    /// Compositor is still running.
    NotShuttingDown,
    /// Compositor is shutting down.
    ShuttingDown,
    /// Compositor has shut down.
    FinishedShuttingDown,
}

/// The paint status of a particular pipeline in the Servo renderer. This is used to trigger metrics
/// in script (via the constellation) when display lists are received.
///
/// See <https://w3c.github.io/paint-timing/#first-contentful-paint>.
#[derive(PartialEq)]
pub(crate) enum PaintMetricState {
    /// The renderer is still waiting to process a display list which triggers this metric.
    Waiting,
    /// The renderer has processed the display list which will trigger this event, marked the Servo
    /// instance ready to paint, and is waiting for the given epoch to actually be rendered.
    Seen(WebRenderEpoch, bool /* first_reflow */),
    /// The metric has been sent to the constellation and no more work needs to be done.
    Sent,
}

struct PipelineDetails {
    /// The pipeline associated with this PipelineDetails object.
    pipeline: Option<CompositionPipeline>,

    /// The id of the parent pipeline, if any.
    parent_pipeline_id: Option<PipelineId>,

    /// The epoch of the most recent display list for this pipeline. Note that this display
    /// list might not be displayed, as WebRender processes display lists asynchronously.
    most_recent_display_list_epoch: Option<WebRenderEpoch>,

    /// Whether animations are running
    animations_running: bool,

    /// Whether there are animation callbacks
    animation_callbacks_running: bool,

    /// Whether to use less resources by stopping animations.
    throttled: bool,

    /// Hit test items for this pipeline. This is used to map WebRender hit test
    /// information to the full information necessary for Servo.
    hit_test_items: Vec<HitTestInfo>,

    /// The compositor-side [ScrollTree]. This is used to allow finding and scrolling
    /// nodes in the compositor before forwarding new offsets to WebRender.
    scroll_tree: ScrollTree,

    /// The paint metric status of the first paint.
    pub first_paint_metric: PaintMetricState,

    /// The paint metric status of the first contentful paint.
    pub first_contentful_paint_metric: PaintMetricState,
}

impl PipelineDetails {
    fn new() -> PipelineDetails {
        PipelineDetails {
            pipeline: None,
            parent_pipeline_id: None,
            most_recent_display_list_epoch: None,
            animations_running: false,
            animation_callbacks_running: false,
            throttled: false,
            hit_test_items: Vec::new(),
            scroll_tree: ScrollTree::default(),
            first_paint_metric: PaintMetricState::Waiting,
            first_contentful_paint_metric: PaintMetricState::Waiting,
        }
    }

    fn install_new_scroll_tree(&mut self, new_scroll_tree: ScrollTree) {
        let old_scroll_offsets: HashMap<ExternalScrollId, LayoutVector2D> = self
            .scroll_tree
            .nodes
            .drain(..)
            .filter_map(|node| match (node.external_id(), node.offset()) {
                (Some(external_id), Some(offset)) => Some((external_id, offset)),
                _ => None,
            })
            .collect();

        self.scroll_tree = new_scroll_tree;
        for node in self.scroll_tree.nodes.iter_mut() {
            match node.external_id() {
                Some(external_id) => match old_scroll_offsets.get(&external_id) {
                    Some(new_offset) => node.set_offset(*new_offset),
                    None => continue,
                },
                _ => continue,
            };
        }
    }
}

impl IOCompositor {
    /// Create a new compositor.
    pub fn new(
        current_window: WindowId,
        viewport: DeviceSize,
        scale_factor: Scale<f32, DeviceIndependentPixel, DevicePixel>,
        state: InitialCompositorState,
        wait_for_stable_image: bool,
        convert_mouse_to_touch: bool,
    ) -> Self {
        let compositor = IOCompositor {
            current_window,
            viewport,
            compositor_receiver: state.receiver,
            webviews: HashMap::new(),
            pipeline_details: HashMap::new(),
            scale_factor,
            composition_request: CompositionRequest::NoCompositingNecessary,
            touch_handler: TouchHandler::new(),
            pending_scroll_zoom_events: Vec::new(),
            shutdown_state: ShutdownState::NotShuttingDown,
            frame_tree_id: FrameTreeId(0),
            constellation_chan: state.constellation_chan,
            time_profiler_chan: state.time_profiler_chan,
            ready_to_save_state: ReadyState::Unknown,
            webrender: Some(state.webrender),
            webrender_document: state.webrender_document,
            webrender_api: state.webrender_api,
            rendering_context: state.rendering_context,
            webrender_gl: state.webrender_gl,
            cursor: Cursor::None,
            cursor_pos: DevicePoint::new(0.0, 0.0),
            wait_for_stable_image,
            convert_mouse_to_touch,
            pending_frames: 0,
            last_animation_tick: Instant::now(),
            is_animating: false,
            ready_to_present: false,
        };

        // Make sure the GL state is OK
        compositor.assert_gl_framebuffer_complete();
        compositor
    }

    /// Consume compositor itself and deinit webrender.
    pub fn deinit(&mut self) {
        if let Some(webrender) = self.webrender.take() {
            webrender.deinit();
        }
    }

    /// Get the current size of the rendering context.
    pub fn rendering_context_size(&self) -> Size2D<u32, DevicePixel> {
        self.rendering_context.size2d()
    }

    pub(crate) fn update_cursor(&mut self, pos: DevicePoint, result: &CompositorHitTestResult) {
        self.cursor_pos = pos;
        let cursor = match result.cursor {
            Some(cursor) if cursor != self.cursor => cursor,
            _ => return,
        };

        let Some(webview_id) = self
            .pipeline_details(result.pipeline_id)
            .pipeline
            .as_ref()
            .map(|composition_pipeline| composition_pipeline.webview_id)
        else {
            warn!(
                "Updating cursor for not-yet-rendered pipeline: {}",
                result.pipeline_id
            );
            return;
        };
        self.cursor = cursor;
        let msg = EmbedderToConstellationMessage::SetCursor(webview_id, cursor);
        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending event to constellation failed ({:?}).", e);
        }
    }

    /// Tell compositor to start shutting down.
    pub fn maybe_start_shutting_down(&mut self) {
        if self.shutdown_state == ShutdownState::NotShuttingDown {
            debug!("Shutting down the constellation for WindowEvent::Quit");
            self.start_shutting_down();
        }
    }

    fn start_shutting_down(&mut self) {
        debug!("Compositor sending Exit message to Constellation");
        if let Err(e) = self
            .constellation_chan
            .send(EmbedderToConstellationMessage::Exit)
        {
            warn!("Sending exit message to constellation failed ({:?}).", e);
        }

        self.shutdown_state = ShutdownState::ShuttingDown;
        self.finish_shutting_down();
    }

    fn finish_shutting_down(&mut self) {
        debug!("Compositor received message that constellation shutdown is complete");

        // Drain compositor port, sometimes messages contain channels that are blocking
        // another thread from finishing (i.e. SetFrameTree).
        while self.compositor_receiver.try_recv().is_ok() {}

        // Tell the profiler, memory profiler, and scrolling timer to shut down.
        if let Ok((sender, receiver)) = ipc::channel() {
            self.time_profiler_chan
                .send(profile_time::ProfilerMsg::Exit(sender));
            let _ = receiver.recv();
        }

        self.shutdown_state = ShutdownState::FinishedShuttingDown;
    }

    fn handle_browser_message(
        &mut self,
        msg: CompositorMsg,
        windows: &mut HashMap<WindowId, (Window, DocumentId)>,
    ) -> bool {
        match self.shutdown_state {
            ShutdownState::NotShuttingDown => {}
            ShutdownState::ShuttingDown => {
                return self.handle_browser_message_while_shutting_down(msg);
            }
            ShutdownState::FinishedShuttingDown => {
                error!("compositor shouldn't be handling messages after shutting down");
                return false;
            }
        }

        match msg {
            CompositorMsg::CollectMemoryReport(sender) => {
                let ops =
                    wr_malloc_size_of::MallocSizeOfOps::new(servo_allocator::usable_size, None);
                let report = self.webrender_api.report_memory(ops);
                let reports = vec![
                    Report {
                        path: path!["webrender", "fonts"],
                        kind: ReportKind::ExplicitJemallocHeapSize,
                        size: report.fonts,
                    },
                    Report {
                        path: path!["webrender", "images"],
                        kind: ReportKind::ExplicitJemallocHeapSize,
                        size: report.images,
                    },
                    Report {
                        path: path!["webrender", "display-list"],
                        kind: ReportKind::ExplicitJemallocHeapSize,
                        size: report.display_list,
                    },
                ];
                sender.send(ProcessReports::new(reports));
            }

            CompositorMsg::ChangeRunningAnimationsState(
                _webview_id,
                pipeline_id,
                animation_state,
            ) => {
                self.change_running_animations_state(pipeline_id, animation_state);
            }

            CompositorMsg::CreateOrUpdateWebView(frame_tree) => {
                self.create_or_update_webview(&frame_tree, windows);
                self.send_scroll_positions_to_layout_for_pipeline(&frame_tree.pipeline.id);
            }

            CompositorMsg::RemoveWebView(webview_id) => {
                self.remove_webview(webview_id, windows);
            }

            CompositorMsg::TouchEventProcessed(_webview_id, result) => {
                self.touch_handler.on_event_processed(result);
            }

            CompositorMsg::CreatePng(_webview_id, _page_rect, reply) => {
                // TODO create image
                if let Err(e) = reply.send(None) {
                    warn!("Sending reply to create png failed ({:?}).", e);
                }
            }

            CompositorMsg::IsReadyToSaveImageReply(is_ready) => {
                assert_eq!(
                    self.ready_to_save_state,
                    ReadyState::WaitingForConstellationReply
                );
                if is_ready && self.pending_frames == 0 {
                    self.ready_to_save_state = ReadyState::ReadyToSaveImage;
                } else {
                    self.ready_to_save_state = ReadyState::Unknown;
                }
                self.composite_if_necessary(CompositingReason::Headless);
            }

            CompositorMsg::SetThrottled(_webview_id, pipeline_id, throttled) => {
                self.pipeline_details(pipeline_id).throttled = throttled;
                self.process_animations(true);
            }

            CompositorMsg::PipelineExited(_webview_id, pipeline_id, sender) => {
                debug!("Compositor got pipeline exited: {:?}", pipeline_id);
                self.remove_pipeline_root_layer(pipeline_id);
                let _ = sender.send(());
            }

            CompositorMsg::NewWebRenderFrameReady(_document_id, recomposite_needed) => {
                self.pending_frames -= 1;

                if recomposite_needed {
                    if let Some(result) = self.hit_test_at_point(self.cursor_pos) {
                        self.update_cursor(self.cursor_pos, &result);
                    }
                }

                if recomposite_needed || self.animation_callbacks_active() {
                    self.composite_if_necessary(CompositingReason::NewWebRenderFrame)
                }
            }

            CompositorMsg::LoadComplete(_) => {
                // If we're painting in headless mode, schedule a recomposite.
                if self.wait_for_stable_image {
                    self.composite_if_necessary(CompositingReason::Headless);
                }
            }

            CompositorMsg::WebDriverMouseButtonEvent(webview_id, action, button, x, y) => {
                let dppx = self.device_pixels_per_page_pixel();
                let point = dppx.transform_point(Point2D::new(x, y));
                self.dispatch_input_event(
                    webview_id,
                    InputEvent::MouseButton(MouseButtonEvent {
                        point,
                        action,
                        button,
                    }),
                );
            }

            CompositorMsg::WebDriverMouseMoveEvent(webview_id, x, y) => {
                let dppx = self.device_pixels_per_page_pixel();
                let point = dppx.transform_point(Point2D::new(x, y));
                self.dispatch_input_event(
                    webview_id,
                    InputEvent::MouseMove(MouseMoveEvent { point }),
                );
            }

            CompositorMsg::SendInitialTransaction(pipeline) => {
                let mut txn = Transaction::new();
                txn.set_display_list(WebRenderEpoch(0), (pipeline, Default::default()));
                self.generate_frame(&mut txn, RenderReasons::SCENE);
                self.webrender_api
                    .send_transaction(self.webrender_document, txn);
            }

            CompositorMsg::SendScrollNode(_webview_id, pipeline_id, point, external_scroll_id) => {
                let pipeline_id = pipeline_id.into();
                let pipeline_details = match self.pipeline_details.get_mut(&pipeline_id) {
                    Some(details) => details,
                    None => return true, // TODO: remove return true after we adapt to api based embder
                };

                let offset = LayoutVector2D::new(point.x, point.y);
                if !pipeline_details
                    .scroll_tree
                    .set_scroll_offsets_for_node_with_external_scroll_id(
                        external_scroll_id,
                        -offset,
                    )
                {
                    warn!("Could not scroll not with id: {external_scroll_id:?}");
                    return true; // TODO: remove return true after we adapt to api based embder
                }

                let mut txn = Transaction::new();
                txn.set_scroll_offsets(
                    external_scroll_id,
                    vec![SampledScrollOffset {
                        offset,
                        generation: 0,
                    }],
                );
                self.generate_frame(&mut txn, RenderReasons::APZ);
                self.webrender_api
                    .send_transaction(self.webrender_document, txn);
            }

            CompositorMsg::SendDisplayList {
                webview_id: _,
                display_list_descriptor,
                display_list_receiver,
            } => {
                // This must match the order from the sender, currently in `shared/script/lib.rs`.
                let display_list_info = match display_list_receiver.recv() {
                    Ok(display_list_info) => display_list_info,
                    Err(error) => {
                        // TODO: remove return true after we adapt to api based embder
                        warn!("Could not receive display list info: {error}");
                        return true;
                    }
                };
                let display_list_info: CompositorDisplayListInfo =
                    match bincode::deserialize(&display_list_info) {
                        Ok(display_list_info) => display_list_info,
                        Err(error) => {
                            // TODO: remove return true after we adapt to api based embder
                            warn!("Could not deserialize display list info: {error}");
                            return true;
                        }
                    };
                let items_data = match display_list_receiver.recv() {
                    Ok(display_list_data) => display_list_data,
                    Err(error) => {
                        // TODO: remove return true after we adapt to api based embder
                        warn!("Could not receive WebRender display list items data: {error}");
                        return true;
                    }
                };
                let cache_data = match display_list_receiver.recv() {
                    Ok(display_list_data) => display_list_data,
                    Err(error) => {
                        // TODO: remove return true after we adapt to api based embder
                        warn!("Could not receive WebRender display list cache data: {error}");
                        return true;
                    }
                };
                let spatial_tree = match display_list_receiver.recv() {
                    Ok(display_list_data) => display_list_data,
                    Err(error) => {
                        // TODO: remove return true after we adapt to api based embder
                        warn!("Could not receive WebRender display list spatial tree: {error}.");
                        return true;
                    }
                };
                let built_display_list = BuiltDisplayList::from_data(
                    DisplayListPayload {
                        items_data,
                        cache_data,
                        spatial_tree,
                    },
                    display_list_descriptor,
                );

                let pipeline_id = display_list_info.pipeline_id;
                let details = self.pipeline_details(pipeline_id.into());
                details.most_recent_display_list_epoch = Some(display_list_info.epoch);
                details.hit_test_items = display_list_info.hit_test_info;
                details.install_new_scroll_tree(display_list_info.scroll_tree);

                let epoch = display_list_info.epoch;
                let first_reflow = display_list_info.first_reflow;
                if details.first_paint_metric == PaintMetricState::Waiting {
                    details.first_paint_metric = PaintMetricState::Seen(epoch, first_reflow);
                }
                if details.first_contentful_paint_metric == PaintMetricState::Waiting
                    && display_list_info.is_contentful
                {
                    details.first_contentful_paint_metric =
                        PaintMetricState::Seen(epoch, first_reflow);
                }

                let mut transaction = Transaction::new();
                transaction
                    .set_display_list(display_list_info.epoch, (pipeline_id, built_display_list));
                self.update_transaction_with_all_scroll_offsets(&mut transaction);
                self.generate_frame(&mut transaction, RenderReasons::SCENE);
                self.webrender_api
                    .send_transaction(self.webrender_document, transaction);
            }

            CompositorMsg::HitTest(pipeline, point, flags, sender) => {
                // When a display list is sent to WebRender, it starts scene building in a
                // separate thread and then that display list is available for hit testing.
                // Without flushing scene building, any hit test we do might be done against
                // a previous scene, if the last one we sent hasn't finished building.
                //
                // TODO(mrobinson): Flushing all scene building is a big hammer here, because
                // we might only be interested in a single pipeline. The only other option
                // would be to listen to the TransactionNotifier for previous per-pipeline
                // transactions, but that isn't easily compatible with the event loop wakeup
                // mechanism from libserver.
                self.webrender_api.flush_scene_builder();

                let result = self.hit_test_at_point_with_flags_and_pipeline(point, flags, pipeline);
                let _ = sender.send(result);
            }

            CompositorMsg::GenerateImageKey(sender) => {
                let _ = sender.send(self.webrender_api.generate_image_key());
            }

            CompositorMsg::UpdateImages(updates) => {
                let mut txn = Transaction::new();
                for update in updates {
                    match update {
                        ImageUpdate::AddImage(key, desc, data) => {
                            txn.add_image(key, desc, data.into(), None)
                        }
                        ImageUpdate::DeleteImage(key) => txn.delete_image(key),
                        ImageUpdate::UpdateImage(key, desc, data) => {
                            txn.update_image(key, desc, data.into(), &DirtyRect::All)
                        }
                    }
                }
                self.webrender_api
                    .send_transaction(self.webrender_document, txn);
            }

            CompositorMsg::AddFont(font_key, data, index) => {
                self.add_font(font_key, index, data);
            }

            CompositorMsg::AddSystemFont(font_key, native_handle) => {
                let mut transaction = Transaction::new();
                transaction.add_native_font(font_key, native_handle);
                self.webrender_api
                    .send_transaction(self.webrender_document, transaction);
            }

            CompositorMsg::AddFontInstance(font_instance_key, font_key, size, flags) => {
                self.add_font_instance(font_instance_key, font_key, size, flags);
            }

            CompositorMsg::RemoveFonts(keys, instance_keys) => {
                let mut transaction = Transaction::new();

                for instance in instance_keys.into_iter() {
                    transaction.delete_font_instance(instance);
                }
                for key in keys.into_iter() {
                    transaction.delete_font(key);
                }

                self.webrender_api
                    .send_transaction(self.webrender_document, transaction);
            }

            CompositorMsg::AddImage(key, desc, data) => {
                let mut txn = Transaction::new();
                txn.add_image(key, desc, data.into(), None);
                self.webrender_api
                    .send_transaction(self.webrender_document, txn);
            }

            CompositorMsg::GenerateFontKeys(
                number_of_font_keys,
                number_of_font_instance_keys,
                result_sender,
            ) => {
                let font_keys = (0..number_of_font_keys)
                    .map(|_| self.webrender_api.generate_font_key())
                    .collect();
                let font_instance_keys = (0..number_of_font_instance_keys)
                    .map(|_| self.webrender_api.generate_font_instance_key())
                    .collect();
                let _ = result_sender.send((font_keys, font_instance_keys));
            }

            CompositorMsg::GetClientWindowRect(_webview_id, response_sender) => {
                // TODO: use ScreenGeometry and bring webviews to compositor. https://github.com/servo/servo/pull/36223
                if let Err(error) =
                    response_sender.send(self.device_independent_int_size_viewport().into())
                {
                    warn!("Sending response to get client window failed ({error:?}).");
                }
            }

            CompositorMsg::GetScreenSize(_webview_id, response_sender) => {
                // TODO: use ScreenGeometry and bring webviews to compositor. https://github.com/servo/servo/pull/36223
                if let Err(error) =
                    response_sender.send(self.device_independent_int_size_viewport())
                {
                    warn!("Sending response to get screen size failed ({error:?}).");
                }
            }

            CompositorMsg::GetAvailableScreenSize(_webview_id, response_sender) => {
                // TODO: use ScreenGeometry and bring webviews to compositor. https://github.com/servo/servo/pull/36223
                if let Err(error) =
                    response_sender.send(self.device_independent_int_size_viewport())
                {
                    warn!("Sending response to get screen size failed ({error:?}).");
                }
            }
        }

        true
    }

    /// Handle messages sent to the compositor during the shutdown process. In general,
    /// the things the compositor can do in this state are limited. It's very important to
    /// answer any synchronous messages though as other threads might be waiting on the
    /// results to finish their own shut down process. We try to do as little as possible
    /// during this time.
    ///
    /// When that involves generating WebRender ids, our approach here is to simply
    /// generate them, but assume they will never be used, since once shutting down the
    /// compositor no longer does any WebRender frame generation.
    fn handle_browser_message_while_shutting_down(&mut self, msg: CompositorMsg) -> bool {
        match msg {
            CompositorMsg::PipelineExited(_webview_id, pipeline_id, sender) => {
                debug!("Compositor got pipeline exited: {:?}", pipeline_id);
                self.remove_pipeline_root_layer(pipeline_id);
                let _ = sender.send(());
            }
            CompositorMsg::GenerateImageKey(sender) => {
                let _ = sender.send(self.webrender_api.generate_image_key());
            }
            CompositorMsg::GenerateFontKeys(
                number_of_font_keys,
                number_of_font_instance_keys,
                result_sender,
            ) => {
                let font_keys = (0..number_of_font_keys)
                    .map(|_| self.webrender_api.generate_font_key())
                    .collect();
                let font_instance_keys = (0..number_of_font_instance_keys)
                    .map(|_| self.webrender_api.generate_font_instance_key())
                    .collect();
                let _ = result_sender.send((font_keys, font_instance_keys));
            }
            CompositorMsg::GetClientWindowRect(_, response_sender) => {
                if let Err(error) = response_sender.send(Default::default()) {
                    warn!("Sending response to get client window failed ({error:?}).");
                }
            }
            CompositorMsg::GetScreenSize(_, response_sender) => {
                if let Err(error) = response_sender.send(Default::default()) {
                    warn!("Sending response to get client window failed ({error:?}).");
                }
            }
            CompositorMsg::GetAvailableScreenSize(_, response_sender) => {
                if let Err(error) = response_sender.send(Default::default()) {
                    warn!("Sending response to get client window failed ({error:?}).");
                }
            }
            CompositorMsg::NewWebRenderFrameReady(..) => {
                // Subtract from the number of pending frames, but do not do any compositing.
                self.pending_frames -= 1;
            }
            _ => {
                debug!("Ignoring message ({:?} while shutting down", msg);
            }
        }
        true
    }

    /// Queue a new frame in the transaction and increase the pending frames count.
    fn generate_frame(&mut self, transaction: &mut Transaction, reason: RenderReasons) {
        self.pending_frames += 1;
        transaction.generate_frame(0, true /* present */, reason);
    }

    /// Sets or unsets the animations-running flag for the given pipeline, and schedules a
    /// recomposite if necessary.
    fn change_running_animations_state(
        &mut self,
        pipeline_id: PipelineId,
        animation_state: AnimationState,
    ) {
        match animation_state {
            AnimationState::AnimationsPresent => {
                let throttled = self.pipeline_details(pipeline_id).throttled;
                self.pipeline_details(pipeline_id).animations_running = true;
                if !throttled {
                    self.composite_if_necessary(CompositingReason::Animation);
                }
            }
            AnimationState::AnimationCallbacksPresent => {
                let throttled = self.pipeline_details(pipeline_id).throttled;
                self.pipeline_details(pipeline_id)
                    .animation_callbacks_running = true;
                if !throttled {
                    self.tick_animations_for_pipeline(pipeline_id);
                }
            }
            AnimationState::NoAnimationsPresent => {
                self.pipeline_details(pipeline_id).animations_running = false;
            }
            AnimationState::NoAnimationCallbacksPresent => {
                self.pipeline_details(pipeline_id)
                    .animation_callbacks_running = false;
            }
        }
    }

    fn pipeline_details(&mut self, pipeline_id: PipelineId) -> &mut PipelineDetails {
        self.pipeline_details
            .entry(pipeline_id)
            .or_insert_with(PipelineDetails::new);
        self.pipeline_details
            .get_mut(&pipeline_id)
            .expect("Insert then get failed!")
    }

    /// Set the root pipeline for our WebRender scene to a display list that consists of an iframe
    /// for each visible top-level browsing context, applying a transformation on the root for
    /// pinch zoom, page zoom, and HiDPI scaling.
    pub fn send_root_pipeline_display_list(&mut self, window: &Window) {
        let mut transaction = Transaction::new();
        self.send_root_pipeline_display_list_in_transaction(&mut transaction, window);
        self.generate_frame(&mut transaction, RenderReasons::SCENE);
        self.webrender_api
            .send_transaction(self.webrender_document, transaction);
    }

    /// Set the root pipeline for our WebRender scene to a display list that consists of an iframe
    /// for each visible top-level browsing context, applying a transformation on the root for
    /// pinch zoom, page zoom, and HiDPI scaling.
    fn send_root_pipeline_display_list_in_transaction(
        &self,
        transaction: &mut Transaction,
        window: &Window,
    ) {
        // Every display list needs a pipeline, but we'd like to choose one that is unlikely
        // to conflict with our content pipelines, which start at (1, 1). (0, 0) is WebRender's
        // dummy pipeline, so we choose (0, 1).
        let root_pipeline = WebRenderPipelineId(u64::from(self.current_window) as u32, 1);
        transaction.set_root_pipeline(root_pipeline);

        let mut builder = webrender::api::DisplayListBuilder::new(root_pipeline);
        builder.begin();

        let zoom_factor = self.device_pixels_per_page_pixel().0;
        let zoom_reference_frame = builder.push_reference_frame(
            LayoutPoint::zero(),
            SpatialId::root_reference_frame(root_pipeline),
            TransformStyle::Flat,
            PropertyBinding::Value(Transform3D::scale(zoom_factor, zoom_factor, 1.)),
            ReferenceFrameKind::Transform {
                is_2d_scale_translation: true,
                should_snap: true,
                paired_with_perspective: false,
            },
            SpatialTreeItemKey::new(0, 0),
        );

        let viewport_size = self.rendering_context.size2d().to_f32().to_untyped();
        let viewport_rect = LayoutRect::from_origin_and_size(
            LayoutPoint::zero(),
            LayoutSize::from_untyped(viewport_size),
        );

        let root_clip_id = builder.define_clip_rect(zoom_reference_frame, viewport_rect);
        let root_clip_chain_id = builder.define_clip_chain(None, [root_clip_id]);
        // Only decorate the webviews if we're in the browser mode
        let should_decorate = window.panel.is_some();
        for webview in window.painting_order() {
            if let Some(pipeline_id) = self.webviews.get(&webview.webview_id) {
                let scaled_webview_rect =
                    LayoutRect::from_untyped(&(webview.rect.to_f32() / zoom_factor).to_untyped());
                let root_space_and_clip = if should_decorate {
                    let complex = ComplexClipRegion::new(
                        scaled_webview_rect,
                        BorderRadius::uniform(10.), // TODO: add fields to webview
                        ClipMode::Clip,
                    );
                    let clip_id = builder.define_clip_rounded_rect(zoom_reference_frame, complex);
                    let clip_chain_id =
                        builder.define_clip_chain(Some(root_clip_chain_id), [clip_id]);
                    SpaceAndClipInfo {
                        spatial_id: zoom_reference_frame,
                        clip_chain_id,
                    }
                } else {
                    SpaceAndClipInfo {
                        spatial_id: zoom_reference_frame,
                        clip_chain_id: root_clip_chain_id,
                    }
                };

                builder.push_iframe(
                    scaled_webview_rect,
                    scaled_webview_rect,
                    &root_space_and_clip,
                    pipeline_id.into(),
                    true,
                );

                if should_decorate {
                    let root_space = SpaceAndClipInfo {
                        spatial_id: zoom_reference_frame,
                        clip_chain_id: root_clip_chain_id,
                    };
                    let offset = vec2(0., 0.);
                    let color = ColorF::new(0.0, 0.0, 0.0, 0.4);
                    let blur_radius = 5.0;
                    let spread_radius = 0.0;
                    let box_shadow_type = BoxShadowClipMode::Outset;

                    builder.push_box_shadow(
                        &CommonItemProperties::new(viewport_rect, root_space),
                        scaled_webview_rect,
                        offset,
                        color,
                        blur_radius,
                        spread_radius,
                        BorderRadius::uniform(10.),
                        box_shadow_type,
                    );
                }
            }
        }

        let built_display_list = builder.end();

        // NB: We are always passing 0 as the epoch here, but this doesn't seem to
        // be an issue. WebRender will still update the scene and generate a new
        // frame even though the epoch hasn't changed.
        transaction.set_display_list(WebRenderEpoch(0), built_display_list);
        self.update_transaction_with_all_scroll_offsets(transaction);
    }

    /// Update the given transaction with the scroll offsets of all active scroll nodes in
    /// the WebRender scene. This is necessary because WebRender does not preserve scroll
    /// offsets between scroll tree modifications. If a display list could potentially
    /// modify a scroll tree branch, WebRender needs to have scroll offsets for that
    /// branch.
    ///
    /// TODO(mrobinson): Could we only send offsets for the branch being modified
    /// and not the entire scene?
    fn update_transaction_with_all_scroll_offsets(&self, transaction: &mut Transaction) {
        for details in self.pipeline_details.values() {
            for node in details.scroll_tree.nodes.iter() {
                let (Some(offset), Some(external_id)) = (node.offset(), node.external_id()) else {
                    continue;
                };

                let offset = LayoutVector2D::new(-offset.x, -offset.y);
                transaction.set_scroll_offsets(
                    external_id,
                    vec![SampledScrollOffset {
                        offset,
                        generation: 0,
                    }],
                );
            }
        }
    }

    fn create_or_update_webview(
        &mut self,
        frame_tree: &SendableFrameTree,
        windows: &mut HashMap<WindowId, (Window, DocumentId)>,
    ) {
        let pipeline_id = frame_tree.pipeline.id;
        let webview_id = frame_tree.pipeline.webview_id;
        debug!(
            "Verso Compositor is setting frame tree with pipeline {} for webview {}",
            pipeline_id, webview_id
        );
        if let Some(old_pipeline) = self.webviews.insert(webview_id, pipeline_id) {
            debug!("{webview_id}'s pipeline has changed from {old_pipeline} to {pipeline_id}");
        }

        if let Some((window, _)) = windows.get(&self.current_window) {
            self.send_root_pipeline_display_list(window);
        }
        self.create_or_update_pipeline_details_with_frame_tree(frame_tree, None);
        self.reset_scroll_tree_for_unattached_pipelines(frame_tree);

        self.frame_tree_id.next();
    }

    fn remove_webview(
        &mut self,
        webview_id: WebViewId,
        windows: &mut HashMap<WindowId, (Window, DocumentId)>,
    ) {
        debug!("Verso Compositor is removing webview {}", webview_id);
        let mut window_id = None;
        for (window, _) in windows.values_mut() {
            let (webview, close_window) = window.remove_webview(webview_id, self);
            if let Some(webview) = webview {
                if let Some(pipeline_id) = self.webviews.remove(&webview.webview_id) {
                    self.remove_pipeline_details_recursively(pipeline_id);
                }

                if close_window {
                    window_id = Some(window.id());
                } else {
                    // if the window is not closed, we need to update the display list
                    // to remove the webview from viewport
                    self.send_root_pipeline_display_list(window);
                }

                self.frame_tree_id.next();
                break;
            }
        }

        if let Some(id) = window_id {
            windows.remove(&id);
        }
    }

    /// Notify compositor the provided webview is resized. The compositor will tell constellation and update the display list.
    pub fn on_resize_webview_event(&mut self, webview_id: WebViewId, rect: DeviceRect) {
        self.send_window_size_message_for_top_level_browser_context(rect, webview_id);
    }

    fn send_window_size_message_for_top_level_browser_context(
        &self,
        rect: DeviceRect,
        webview_id: WebViewId,
    ) {
        // The device pixel ratio used by the style system should include the scale from page pixels
        // to device pixels, but not including any pinch zoom.
        let hidpi_scale_factor = self.device_pixels_per_page_pixel_not_including_page_zoom();
        let size = rect.size().to_f32() / hidpi_scale_factor;
        let msg = EmbedderToConstellationMessage::ChangeViewportDetails(
            webview_id,
            ViewportDetails {
                size,
                hidpi_scale_factor,
            },
            WindowSizeType::Resize,
        );
        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending window resize to constellation failed ({:?}).", e);
        }
    }

    fn reset_scroll_tree_for_unattached_pipelines(&mut self, frame_tree: &SendableFrameTree) {
        // TODO(mrobinson): Eventually this can selectively preserve the scroll trees
        // state for some unattached pipelines in order to preserve scroll position when
        // navigating backward and forward.
        fn collect_pipelines(pipelines: &mut HashSet<PipelineId>, frame_tree: &SendableFrameTree) {
            pipelines.insert(frame_tree.pipeline.id);
            for kid in &frame_tree.children {
                collect_pipelines(pipelines, kid);
            }
        }

        let mut attached_pipelines = HashSet::default();
        collect_pipelines(&mut attached_pipelines, frame_tree);

        self.pipeline_details
            .iter_mut()
            .filter(|(id, _)| !attached_pipelines.contains(id))
            .for_each(|(_, details)| {
                details.scroll_tree.nodes.iter_mut().for_each(|node| {
                    node.set_offset(LayoutVector2D::zero());
                })
            })
    }

    fn create_or_update_pipeline_details_with_frame_tree(
        &mut self,
        frame_tree: &SendableFrameTree,
        parent_pipeline_id: Option<PipelineId>,
    ) {
        let pipeline_id = frame_tree.pipeline.id;
        let pipeline_details = self.pipeline_details(pipeline_id);
        pipeline_details.pipeline = Some(frame_tree.pipeline.clone());
        pipeline_details.parent_pipeline_id = parent_pipeline_id;

        for kid in &frame_tree.children {
            self.create_or_update_pipeline_details_with_frame_tree(kid, Some(pipeline_id));
        }
    }

    fn remove_pipeline_details_recursively(&mut self, pipeline_id: PipelineId) {
        self.pipeline_details.remove(&pipeline_id);

        let children = self
            .pipeline_details
            .iter()
            .filter(|(_, pipeline_details)| {
                pipeline_details.parent_pipeline_id == Some(pipeline_id)
            })
            .map(|(&pipeline_id, _)| pipeline_id)
            .collect::<Vec<_>>();

        for kid in children {
            self.remove_pipeline_details_recursively(kid);
        }
    }

    fn remove_pipeline_root_layer(&mut self, pipeline_id: PipelineId) {
        self.pipeline_details.remove(&pipeline_id);
    }

    /// Change the current window of the compositor should display.
    pub fn swap_current_window(&mut self, window: &mut Window) {
        if window.id() != self.current_window {
            debug!(
                "Verso Compositor swap current window from {:?} to {:?}",
                self.current_window,
                window.id()
            );
            self.current_window = window.id();
            self.scale_factor = Scale::new(window.scale_factor() as f32);
            self.resize(window.size(), window);
        }
    }

    /// Resize the rendering context and all web views.
    pub fn resize(&mut self, size: Size2D<f32, DevicePixel>, window: &mut Window) {
        if size.height == 0.0 || size.width == 0.0 {
            return;
        }

        self.on_resize_window_event(size, window);

        if let Some(panel) = &mut window.panel {
            let rect = DeviceRect::from_size(size);
            panel.webview.rect = rect;
            self.on_resize_webview_event(panel.webview.webview_id, rect);
        }

        let rect = DeviceRect::from_size(size);
        let show_tab_bar = window.tab_manager.count() > 1;
        let content_size = window.get_content_size(rect, show_tab_bar, window.show_bookmark);
        if let Some(tab_id) = window.tab_manager.current_tab_id() {
            let (tab_id, prompt_id) = window.tab_manager.set_size(tab_id, content_size);
            if let Some(tab_id) = tab_id {
                self.on_resize_webview_event(tab_id, content_size);
            }
            if let Some(prompt_id) = prompt_id {
                self.on_resize_webview_event(prompt_id, content_size);
            }
        }
        #[cfg(linux)]
        if let Some(webview_menu) = &mut window.webview_menu {
            let rect = DeviceRect::from_size(size);
            webview_menu.set_webview_rect(rect);
            self.on_resize_webview_event(webview_menu.webview().webview_id, rect);
        }

        self.send_root_pipeline_display_list(window);
    }

    /// Handle the window resize event.
    pub fn on_resize_window_event(&mut self, new_viewport: DeviceSize, window: &Window) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        self.rendering_context.resize(
            &window.surface,
            PhysicalSize {
                width: new_viewport.width as u32,
                height: new_viewport.height as u32,
            },
        );
        self.viewport = new_viewport;
        let mut transaction = Transaction::new();
        transaction.set_document_view(DeviceIntRect::from_size(self.viewport.to_i32()));
        self.webrender_api
            .send_transaction(self.webrender_document, transaction);
        self.composite_if_necessary(CompositingReason::Resize);
    }

    /// Handle the window scale factor event and return a boolean to tell embedder if it should further
    /// handle the scale factor event.
    pub fn on_scale_factor_event(&mut self, scale_factor: f32, window: &Window) -> bool {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return false;
        }

        self.scale_factor = Scale::new(scale_factor);
        self.update_after_zoom_or_hidpi_change(window);
        self.composite_if_necessary(CompositingReason::Resize);
        true
    }

    /// Dispatch input event to constellation.
    fn dispatch_input_event(&mut self, webview_id: WebViewId, event: InputEvent) {
        // Events that do not need to do hit testing are sent directly to the
        // constellation to filter down.
        let Some(point) = event.point() else {
            return;
        };

        // If we can't find a pipeline to send this event to, we cannot continue.
        let Some(result) = self.hit_test_at_point(point) else {
            return;
        };

        self.update_cursor(point, &result);

        if let Err(error) =
            self.constellation_chan
                .send(EmbedderToConstellationMessage::ForwardInputEvent(
                    webview_id,
                    event.clone(),
                    Some(result),
                ))
        {
            warn!("Sending event to constellation failed ({error:?}).");
        }

        // If the event is a mouse button, send FocusWebView event to the constellation.
        // the webview will update current focused webview_id in the EmbedderMsg::WebViewFocused event.
        if let InputEvent::MouseButton(event) = &event {
            if event.action == MouseButtonAction::Click {
                let _ = self
                    .constellation_chan
                    .send(EmbedderToConstellationMessage::FocusWebView(webview_id));
            }
        }
    }

    /// Handle the input event in the window.
    pub fn on_input_event(&mut self, webview_id: WebViewId, event: InputEvent) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }
        if self.convert_mouse_to_touch {
            match event {
                InputEvent::MouseButton(event) => {
                    match event.action {
                        MouseButtonAction::Click => {}
                        MouseButtonAction::Down => self.on_touch_down(
                            webview_id,
                            TouchEvent::new(TouchEventType::Down, TouchId(0), event.point),
                        ),
                        MouseButtonAction::Up => self.on_touch_up(
                            webview_id,
                            TouchEvent::new(TouchEventType::Up, TouchId(0), event.point),
                        ),
                    }
                    return;
                }
                InputEvent::MouseMove(event) => {
                    self.on_touch_move(
                        webview_id,
                        TouchEvent::new(TouchEventType::Move, TouchId(0), event.point),
                    );
                    return;
                }
                _ => {}
            }
        }
        self.dispatch_input_event(webview_id, event);
    }

    /// Get the webview id from a point.
    pub(crate) fn webview_id_from_point(&self, point: DevicePoint) -> Option<WebViewId> {
        self.hit_test_at_point(point)
            .map(|result| result.pipeline_id)
            .and_then(|pipeline_id| self.pipeline_details.get(&pipeline_id))
            .and_then(|details| details.pipeline.clone())
            .map(|pipeline| pipeline.webview_id)
    }

    fn hit_test_at_point(&self, point: DevicePoint) -> Option<CompositorHitTestResult> {
        self.hit_test_at_point_with_flags_and_pipeline(point, HitTestFlags::empty(), None)
            .first()
            .cloned()
    }

    fn hit_test_at_point_with_flags_and_pipeline(
        &self,
        point: DevicePoint,
        flags: HitTestFlags,
        pipeline_id: Option<WebRenderPipelineId>,
    ) -> Vec<CompositorHitTestResult> {
        // DevicePoint and WorldPoint are the same for us.
        let world_point = WorldPoint::from_untyped(point.to_untyped());
        let results =
            self.webrender_api
                .hit_test(self.webrender_document, pipeline_id, world_point, flags);

        results
            .items
            .iter()
            .filter_map(|item| {
                let pipeline_id = item.pipeline.into();
                let details = match self.pipeline_details.get(&pipeline_id) {
                    Some(details) => details,
                    None => return None,
                };

                // If the epoch in the tag does not match the current epoch of the pipeline,
                // then the hit test is against an old version of the display list and we
                // should ignore this hit test for now.
                match details.most_recent_display_list_epoch {
                    Some(epoch) if epoch.as_u16() == item.tag.1 => {}
                    _ => return None,
                }

                let info = &details.hit_test_items[item.tag.0 as usize];
                Some(CompositorHitTestResult {
                    pipeline_id,
                    point_in_viewport: item.point_in_viewport.to_untyped(),
                    point_relative_to_item: item.point_relative_to_item.to_untyped(),
                    node: UntrustedNodeAddress(info.node as *const c_void),
                    cursor: info.cursor,
                    scroll_tree_node: info.scroll_tree_node,
                })
            })
            .collect()
    }

    fn send_touch_event(&self, webview_id: WebViewId, event: TouchEvent) {
        let Some(result) = self.hit_test_at_point(event.point) else {
            return;
        };

        let event = InputEvent::Touch(event);
        if let Err(e) =
            self.constellation_chan
                .send(EmbedderToConstellationMessage::ForwardInputEvent(
                    webview_id,
                    event,
                    Some(result),
                ))
        {
            warn!("Sending event to constellation failed ({:?}).", e);
        }
    }

    /// Handle touch event.
    pub fn on_touch_event(&mut self, webview_id: WebViewId, event: TouchEvent) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        match event.event_type {
            TouchEventType::Down => self.on_touch_down(webview_id, event),
            TouchEventType::Move => self.on_touch_move(webview_id, event),
            TouchEventType::Up => self.on_touch_up(webview_id, event),
            TouchEventType::Cancel => self.on_touch_cancel(webview_id, event),
        }
    }

    fn on_touch_down(&mut self, webview_id: WebViewId, event: TouchEvent) {
        self.touch_handler.on_touch_down(event.id, event.point);
        self.send_touch_event(webview_id, event);
    }

    fn on_touch_move(&mut self, webview_id: WebViewId, event: TouchEvent) {
        match self.touch_handler.on_touch_move(event.id, event.point) {
            TouchAction::Scroll(delta) => self.on_scroll_window_event(
                ScrollLocation::Delta(LayoutVector2D::from_untyped(delta.to_untyped())),
                event.point.cast(),
            ),
            TouchAction::Zoom(magnification, scroll_delta) => {
                let cursor = Point2D::new(-1, -1); // Make sure this hits the base layer.

                // The order of these events doesn't matter, because zoom is handled by
                // a root display list and the scroll event here is handled by the scroll
                // applied to the content display list.
                self.pending_scroll_zoom_events
                    .push(ScrollZoomEvent::PinchZoom(magnification));
                self.pending_scroll_zoom_events
                    .push(ScrollZoomEvent::Scroll(ScrollEvent {
                        scroll_location: ScrollLocation::Delta(LayoutVector2D::from_untyped(
                            scroll_delta.to_untyped(),
                        )),
                        cursor,
                        event_count: 1,
                    }));
            }
            TouchAction::DispatchEvent => self.send_touch_event(webview_id, event),
            _ => {}
        }
    }

    fn on_touch_up(&mut self, webview_id: WebViewId, event: TouchEvent) {
        self.send_touch_event(webview_id, event);

        if let TouchAction::Click = self.touch_handler.on_touch_up(event.id, event.point) {
            self.simulate_mouse_click(webview_id, event.point);
        }
    }

    fn on_touch_cancel(&mut self, webview_id: WebViewId, event: TouchEvent) {
        // Send the event to script.
        self.touch_handler.on_touch_cancel(event.id, event.point);
        self.send_touch_event(webview_id, event);
    }

    /// <http://w3c.github.io/touch-events/#mouse-events>
    fn simulate_mouse_click(&mut self, webview_id: WebViewId, point: DevicePoint) {
        let button = MouseButton::Left;
        self.dispatch_input_event(webview_id, InputEvent::MouseMove(MouseMoveEvent { point }));
        self.dispatch_input_event(
            webview_id,
            InputEvent::MouseButton(MouseButtonEvent {
                button,
                action: MouseButtonAction::Down,
                point,
            }),
        );
        self.dispatch_input_event(
            webview_id,
            InputEvent::MouseButton(MouseButtonEvent {
                button,
                action: MouseButtonAction::Up,
                point,
            }),
        );
        self.dispatch_input_event(
            webview_id,
            InputEvent::MouseButton(MouseButtonEvent {
                button,
                action: MouseButtonAction::Click,
                point,
            }),
        );
    }

    /// Handle scroll event.
    pub fn on_scroll_event(
        &mut self,
        scroll_location: ScrollLocation,
        cursor: DeviceIntPoint,
        action: TouchEventType,
    ) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        match action {
            TouchEventType::Move => self.on_scroll_window_event(scroll_location, cursor),
            TouchEventType::Up | TouchEventType::Cancel => {
                self.on_scroll_window_event(scroll_location, cursor);
            }
            TouchEventType::Down => {
                self.on_scroll_window_event(scroll_location, cursor);
            }
        }
    }

    fn on_scroll_window_event(&mut self, scroll_location: ScrollLocation, cursor: DeviceIntPoint) {
        self.pending_scroll_zoom_events
            .push(ScrollZoomEvent::Scroll(ScrollEvent {
                scroll_location,
                cursor,
                event_count: 1,
            }));
    }

    fn process_pending_scroll_events(&mut self, _window: &Window) {
        // Batch up all scroll events into one, or else we'll do way too much painting.
        let mut combined_scroll_event: Option<ScrollEvent> = None;
        let mut _combined_magnification = 1.0;
        for scroll_event in self.pending_scroll_zoom_events.drain(..) {
            match scroll_event {
                ScrollZoomEvent::PinchZoom(magnification) => {
                    _combined_magnification *= magnification
                }
                ScrollZoomEvent::Scroll(scroll_event_info) => {
                    let combined_event = match combined_scroll_event.as_mut() {
                        None => {
                            combined_scroll_event = Some(scroll_event_info);
                            continue;
                        }
                        Some(combined_event) => combined_event,
                    };

                    match (
                        combined_event.scroll_location,
                        scroll_event_info.scroll_location,
                    ) {
                        (ScrollLocation::Delta(old_delta), ScrollLocation::Delta(new_delta)) => {
                            // Mac OS X sometimes delivers scroll events out of vsync during a
                            // fling. This causes events to get bunched up occasionally, causing
                            // nasty-looking "pops". To mitigate this, during a fling we average
                            // deltas instead of summing them.
                            let old_event_count = Scale::new(combined_event.event_count as f32);
                            combined_event.event_count += 1;
                            let new_event_count = Scale::new(combined_event.event_count as f32);
                            combined_event.scroll_location = ScrollLocation::Delta(
                                (old_delta * old_event_count + new_delta) / new_event_count,
                            );
                        }
                        (ScrollLocation::Start, _) | (ScrollLocation::End, _) => {
                            // Once we see Start or End, we shouldn't process any more events.
                            break;
                        }
                        (_, ScrollLocation::Start) | (_, ScrollLocation::End) => {
                            // If this is an event which is scrolling to the start or end of the page,
                            // disregard other pending events and exit the loop.
                            *combined_event = scroll_event_info;
                            break;
                        }
                    }
                }
            }
        }

        let scroll_result = combined_scroll_event.and_then(|combined_event| {
            self.scroll_node_at_device_point(
                combined_event.cursor.to_f32(),
                combined_event.scroll_location,
            )
        });

        let mut transaction = Transaction::new();

        if let Some((pipeline_id, external_id, offset)) = scroll_result {
            let offset = LayoutVector2D::new(-offset.x, -offset.y);
            transaction.set_scroll_offsets(
                external_id,
                vec![SampledScrollOffset {
                    offset,
                    generation: 0,
                }],
            );
            self.send_scroll_positions_to_layout_for_pipeline(&pipeline_id);
        }

        self.generate_frame(&mut transaction, RenderReasons::APZ);
        self.webrender_api
            .send_transaction(self.webrender_document, transaction);
    }

    /// Perform a hit test at the given [`DevicePoint`] and apply the [`ScrollLocation`]
    /// scrolling to the applicable scroll node under that point. If a scroll was
    /// performed, returns the [`PipelineId`] of the node scrolled, the id, and the final
    /// scroll delta.
    fn scroll_node_at_device_point(
        &mut self,
        cursor: DevicePoint,
        scroll_location: ScrollLocation,
    ) -> Option<(PipelineId, ExternalScrollId, LayoutVector2D)> {
        let scroll_location = match scroll_location {
            ScrollLocation::Delta(delta) => {
                let device_pixels_per_page = self.device_pixels_per_page_pixel();
                let scaled_delta = (Vector2D::from_untyped(delta.to_untyped())
                    / device_pixels_per_page)
                    .to_untyped();
                let calculated_delta = LayoutVector2D::from_untyped(scaled_delta);
                ScrollLocation::Delta(calculated_delta)
            }
            // Leave ScrollLocation unchanged if it is Start or End location.
            ScrollLocation::Start | ScrollLocation::End => scroll_location,
        };

        let hit_test_results =
            self.hit_test_at_point_with_flags_and_pipeline(cursor, HitTestFlags::FIND_ALL, None);

        // Iterate through all hit test results, processing only the first node of each pipeline.
        // This is needed to propagate the scroll events from a pipeline representing an iframe to
        // its ancestor pipelines.
        let mut previous_pipeline_id = None;
        for CompositorHitTestResult {
            pipeline_id,
            scroll_tree_node,
            ..
        } in hit_test_results.iter()
        {
            if previous_pipeline_id.replace(pipeline_id) != Some(pipeline_id) {
                let scroll_result = self
                    .pipeline_details
                    .get_mut(pipeline_id)?
                    .scroll_tree
                    .scroll_node_or_ancestor(scroll_tree_node, scroll_location);
                if let Some((external_id, offset)) = scroll_result {
                    return Some((*pipeline_id, external_id, offset));
                }
            }
        }
        None
    }

    /// If there are any animations running, dispatches appropriate messages to the constellation.
    fn process_animations(&mut self, force: bool) {
        // When running animations in order to dump a screenshot (not after a full composite), don't send
        // animation ticks faster than about 60Hz.
        //
        // TODO: This should be based on the refresh rate of the screen and also apply to all
        // animation ticks, not just ones sent while waiting to dump screenshots. This requires
        // something like a refresh driver concept though.
        if !force && (Instant::now() - self.last_animation_tick) < Duration::from_millis(16) {
            return;
        }
        self.last_animation_tick = Instant::now();

        let mut pipeline_ids = vec![];
        for (pipeline_id, pipeline_details) in &self.pipeline_details {
            if (pipeline_details.animations_running || pipeline_details.animation_callbacks_running)
                && !pipeline_details.throttled
            {
                pipeline_ids.push(*pipeline_id);
            }
        }
        self.is_animating = !pipeline_ids.is_empty();
        for pipeline_id in &pipeline_ids {
            self.tick_animations_for_pipeline(*pipeline_id)
        }
    }

    fn tick_animations_for_pipeline(&mut self, pipeline_id: PipelineId) {
        let animation_callbacks_running = self
            .pipeline_details(pipeline_id)
            .animation_callbacks_running;
        let animations_running = self.pipeline_details(pipeline_id).animations_running;
        if !animation_callbacks_running && !animations_running {
            return;
        }

        let mut tick_type = AnimationTickType::empty();
        if animations_running {
            tick_type.insert(AnimationTickType::CSS_ANIMATIONS_AND_TRANSITIONS);
        }
        if animation_callbacks_running {
            tick_type.insert(AnimationTickType::REQUEST_ANIMATION_FRAME);
        }

        let msg = EmbedderToConstellationMessage::TickAnimation(pipeline_id, tick_type);
        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending tick to constellation failed ({:?}).", e);
        }
    }

    fn device_pixels_per_page_pixel(&self) -> Scale<f32, CSSPixel, DevicePixel> {
        self.device_pixels_per_page_pixel_not_including_page_zoom()
    }

    fn device_pixels_per_page_pixel_not_including_page_zoom(
        &self,
    ) -> Scale<f32, CSSPixel, DevicePixel> {
        Scale::new(self.scale_factor.get())
    }

    fn device_independent_int_size_viewport(&self) -> DeviceIndependentIntSize {
        (self.viewport.to_f32() / self.scale_factor).to_i32()
    }

    /// Handle zoom reset event
    pub fn on_zoom_reset_window_event(&mut self, window: &Window) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        self.update_after_zoom_or_hidpi_change(window);
    }

    /// Handle zoom event in the window
    pub fn on_zoom_window_event(&mut self, _magnification: f32, window: &Window) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        self.update_after_zoom_or_hidpi_change(window);
    }

    fn update_after_zoom_or_hidpi_change(&mut self, window: &Window) {
        for webview in window.painting_order() {
            self.send_window_size_message_for_top_level_browser_context(
                webview.rect,
                webview.webview_id,
            );
        }

        // Update the root transform in WebRender to reflect the new zoom.
        self.send_root_pipeline_display_list(window);
    }

    /// Simulate a pinch zoom
    pub fn on_pinch_zoom_window_event(&mut self, magnification: f32) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        // TODO: Scroll to keep the center in view?
        self.pending_scroll_zoom_events
            .push(ScrollZoomEvent::PinchZoom(magnification));
    }

    fn send_scroll_positions_to_layout_for_pipeline(&self, pipeline_id: &PipelineId) {
        let details = match self.pipeline_details.get(pipeline_id) {
            Some(details) => details,
            None => return,
        };

        let mut scroll_states = Vec::new();
        details.scroll_tree.nodes.iter().for_each(|node| {
            if let (Some(scroll_id), Some(scroll_offset)) = (node.external_id(), node.offset()) {
                scroll_states.push(ScrollState {
                    scroll_id,
                    scroll_offset,
                });
            }
        });

        let message = EmbedderToConstellationMessage::SetScrollStates(*pipeline_id, scroll_states);
        let _ = self.constellation_chan.send(message);
    }

    // Check if any pipelines currently have active animations or animation callbacks.
    fn animations_active(&self) -> bool {
        for details in self.pipeline_details.values() {
            // If animations are currently running, then don't bother checking
            // with the constellation if the output image is stable.
            if details.animations_running {
                return true;
            }
            if details.animation_callbacks_running {
                return true;
            }
        }

        false
    }

    /// Returns true if any animation callbacks (ie `requestAnimationFrame`) are waiting for a response.
    fn animation_callbacks_active(&self) -> bool {
        self.pipeline_details
            .values()
            .any(|details| details.animation_callbacks_running)
    }

    /// Query the constellation to see if the current compositor
    /// output matches the current frame tree output, and if the
    /// associated script threads are idle.
    fn is_ready_to_paint_image_output(&mut self) -> Result<(), NotReadyToPaint> {
        match self.ready_to_save_state {
            ReadyState::Unknown => {
                // Unsure if the output image is stable.

                // Collect the currently painted epoch of each pipeline that is
                // complete (i.e. has *all* layers painted to the requested epoch).
                // This gets sent to the constellation for comparison with the current
                // frame tree.
                let mut pipeline_epochs = HashMap::new();
                for id in self.pipeline_details.keys() {
                    if let Some(WebRenderEpoch(epoch)) = self
                        .webrender
                        .as_ref()
                        .and_then(|wr| wr.current_epoch(self.webrender_document, id.into()))
                    {
                        let epoch = Epoch(epoch);
                        pipeline_epochs.insert(*id, epoch);
                    }
                }

                // Pass the pipeline/epoch states to the constellation and check
                // if it's safe to output the image.
                let msg = EmbedderToConstellationMessage::IsReadyToSaveImage(pipeline_epochs);
                if let Err(e) = self.constellation_chan.send(msg) {
                    warn!("Sending ready to save to constellation failed ({:?}).", e);
                }
                self.ready_to_save_state = ReadyState::WaitingForConstellationReply;
                Err(NotReadyToPaint::JustNotifiedConstellation)
            }
            ReadyState::WaitingForConstellationReply => {
                // If waiting on a reply from the constellation to the last
                // query if the image is stable, then assume not ready yet.
                Err(NotReadyToPaint::WaitingOnConstellation)
            }
            ReadyState::ReadyToSaveImage => {
                // Constellation has replied at some point in the past
                // that the current output image is stable and ready
                // for saving.
                // Reset the flag so that we check again in the future
                // TODO: only reset this if we load a new document?
                self.ready_to_save_state = ReadyState::Unknown;
                Ok(())
            }
        }
    }

    /// Composite to the given target if any, or the current target otherwise.
    pub fn composite(&mut self, window: &Window) {
        match self.composite_specific_target(window) {
            Ok(_) => {
                if self.wait_for_stable_image {
                    println!(
                        "Shutting down the Constellation after generating an output file or exit flag specified"
                    );
                    self.start_shutting_down();
                }
            }
            Err(error) => {
                trace!("Unable to composite: {error:?}");
            }
        }
    }

    /// Composite to the given target if any, or the current target otherwise.
    fn composite_specific_target(&mut self, window: &Window) -> Result<(), UnableToComposite> {
        if let Err(err) = self
            .rendering_context
            .make_gl_context_current(&window.surface)
        {
            warn!("Failed to make GL context current: {:?}", err);
        }
        self.assert_no_gl_error();

        if let Some(webrender) = self.webrender.as_mut() {
            webrender.update();
        }

        let wait_for_stable_image = self.wait_for_stable_image;

        if wait_for_stable_image {
            // The current image may be ready to output. However, if there are animations active,
            // tick those instead and continue waiting for the image output to be stable AND
            // all active animations to complete.
            if self.animations_active() {
                self.process_animations(false);
                return Err(UnableToComposite::NotReadyToPaintImage(
                    NotReadyToPaint::AnimationsActive,
                ));
            }
            if let Err(result) = self.is_ready_to_paint_image_output() {
                return Err(UnableToComposite::NotReadyToPaintImage(result));
            }
        }

        time_profile!(
            ProfilerCategory::Compositing,
            None,
            self.time_profiler_chan.clone(),
            || {
                trace!("Compositing");
                // Paint the scene.
                // TODO(gw): Take notice of any errors the renderer returns!
                if let Some(webrender) = self.webrender.as_mut() {
                    webrender
                        .render(self.viewport.to_i32(), 0 /* buffer_age */)
                        .ok();
                }
            },
        );

        self.send_pending_paint_metrics_messages_after_composite();

        self.composition_request = CompositionRequest::NoCompositingNecessary;
        self.ready_to_present = true;

        self.process_animations(true);

        Ok(())
    }

    fn composite_if_necessary(&mut self, reason: CompositingReason) {
        trace!(
            "Will schedule a composite {reason:?}. Previously was {:?}",
            self.composition_request
        );
        self.composition_request = CompositionRequest::CompositeNow(reason)
    }

    #[track_caller]
    fn assert_no_gl_error(&self) {
        debug_assert_eq!(self.webrender_gl.get_error(), gl::NO_ERROR);
    }

    #[track_caller]
    fn assert_gl_framebuffer_complete(&self) {
        debug_assert_eq!(
            (
                self.webrender_gl.get_error(),
                self.webrender_gl.check_frame_buffer_status(gl::FRAMEBUFFER)
            ),
            (gl::NO_ERROR, gl::FRAMEBUFFER_COMPLETE)
        );
    }

    /// Receive and handle compositor messages.
    pub fn receive_messages(
        &mut self,
        windows: &mut HashMap<WindowId, (Window, DocumentId)>,
    ) -> bool {
        // Check for new messages coming from the other threads in the system.
        let mut compositor_messages = vec![];
        let mut found_recomposite_msg = false;
        while let Ok(msg) = self.compositor_receiver.try_recv() {
            match msg {
                CompositorMsg::NewWebRenderFrameReady(..) if found_recomposite_msg => {
                    // Only take one of duplicate NewWebRendeFrameReady messages, but do subtract
                    // one frame from the pending frames.
                    self.pending_frames -= 1;
                }
                CompositorMsg::NewWebRenderFrameReady(..) => {
                    found_recomposite_msg = true;
                    compositor_messages.push(msg)
                }
                _ => compositor_messages.push(msg),
            }
        }
        for msg in compositor_messages {
            if !self.handle_browser_message(msg, windows) {
                return false;
            }
        }
        true
    }

    /// Perform composition and related actions.
    pub fn perform_updates(
        &mut self,
        windows: &mut HashMap<WindowId, (Window, DocumentId)>,
    ) -> bool {
        if self.shutdown_state == ShutdownState::FinishedShuttingDown {
            return false;
        }

        if let Some((window, _)) = windows.get(&self.current_window) {
            match self.composition_request {
                CompositionRequest::NoCompositingNecessary => {}
                CompositionRequest::CompositeNow(_) => {
                    self.composite(window);
                    window.request_redraw();
                }
            }

            if !self.pending_scroll_zoom_events.is_empty() {
                self.process_pending_scroll_events(window)
            }
        }
        self.shutdown_state != ShutdownState::FinishedShuttingDown
    }

    /// Update debug option of the webrender.
    pub fn toggle_webrender_debug(&mut self, option: WebRenderDebugOption) {
        let Some(webrender) = self.webrender.as_mut() else {
            return;
        };
        let mut flags = webrender.get_debug_flags();
        let flag = match option {
            WebRenderDebugOption::Profiler => {
                webrender::DebugFlags::PROFILER_DBG
                    | webrender::DebugFlags::GPU_TIME_QUERIES
                    | webrender::DebugFlags::GPU_SAMPLE_QUERIES
            }
            WebRenderDebugOption::TextureCacheDebug => webrender::DebugFlags::TEXTURE_CACHE_DBG,
            WebRenderDebugOption::RenderTargetDebug => webrender::DebugFlags::RENDER_TARGET_DBG,
        };
        flags.toggle(flag);
        webrender.set_debug_flags(flags);

        let mut txn = Transaction::new();
        self.generate_frame(&mut txn, RenderReasons::TESTING);
        self.webrender_api
            .send_transaction(self.webrender_document, txn);
    }

    fn add_font_instance(
        &mut self,
        instance_key: FontInstanceKey,
        font_key: FontKey,
        size: f32,
        flags: FontInstanceFlags,
    ) {
        let mut transaction = Transaction::new();
        let font_instance_options = FontInstanceOptions {
            flags,
            ..Default::default()
        };
        transaction.add_font_instance(
            instance_key,
            font_key,
            size,
            Some(font_instance_options),
            None,
            Vec::new(),
        );
        self.webrender_api
            .send_transaction(self.webrender_document, transaction);
    }

    fn add_font(&mut self, font_key: FontKey, index: u32, data: Arc<IpcSharedMemory>) {
        let mut transaction = Transaction::new();
        transaction.add_raw_font(font_key, (**data).into(), index);
        self.webrender_api
            .send_transaction(self.webrender_document, transaction);
    }

    /// Send all pending paint metrics messages after a composite operation, which may advance
    /// the epoch for pipelines in the WebRender scene.
    ///
    /// If there are pending paint metrics, we check if any of the painted epochs is one
    /// of the ones that the paint metrics recorder is expecting. In that case, we get the
    /// current time, inform the constellation about it and remove the pending metric from
    /// the list.
    fn send_pending_paint_metrics_messages_after_composite(&mut self) {
        let paint_time = CrossProcessInstant::now();
        let document_id = self.webrender_document;
        for (_, pipeline_id) in self.webviews.iter_mut() {
            debug_assert!(self.pipeline_details.contains_key(pipeline_id));
            let pipeline = self.pipeline_details.get_mut(pipeline_id).unwrap();
            let Some(current_epoch) = self
                .webrender
                .as_ref()
                .and_then(|wr| wr.current_epoch(document_id, (*pipeline_id).into()))
            else {
                continue;
            };

            match pipeline.first_paint_metric {
                // We need to check whether the current epoch is later, because
                // CompositorMsg::SendInitialTransaction sends an
                // empty display list to WebRender which can happen before we receive
                // the first "real" display list.
                PaintMetricState::Seen(epoch, first_reflow) if epoch <= current_epoch => {
                    assert!(epoch <= current_epoch);
                    if let Err(error) =
                        self.constellation_chan
                            .send(EmbedderToConstellationMessage::PaintMetric(
                                *pipeline_id,
                                PaintMetricEvent::FirstPaint(paint_time, first_reflow),
                            ))
                    {
                        warn!("Sending paint metric event to constellation failed ({error:?}).");
                    }
                    pipeline.first_paint_metric = PaintMetricState::Sent;
                }
                _ => {}
            }

            match pipeline.first_contentful_paint_metric {
                PaintMetricState::Seen(epoch, first_reflow) if epoch <= current_epoch => {
                    if let Err(error) =
                        self.constellation_chan
                            .send(EmbedderToConstellationMessage::PaintMetric(
                                *pipeline_id,
                                PaintMetricEvent::FirstContentfulPaint(paint_time, first_reflow),
                            ))
                    {
                        warn!("Sending paint metric event to constellation failed ({error:?}).");
                    }
                    pipeline.first_contentful_paint_metric = PaintMetricState::Sent;
                }
                _ => {}
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum UnableToComposite {
    NotReadyToPaintImage(NotReadyToPaint),
}

#[derive(Debug, PartialEq)]
enum NotReadyToPaint {
    AnimationsActive,
    JustNotifiedConstellation,
    WaitingOnConstellation,
}

/// Holds the state when running reftests that determines when it is
/// safe to save the output image.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ReadyState {
    Unknown,
    WaitingForConstellationReply,
    ReadyToSaveImage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FrameTreeId(u32);

impl FrameTreeId {
    pub fn next(&mut self) {
        self.0 += 1;
    }
}
