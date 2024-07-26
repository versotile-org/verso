use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use compositing_traits::{
    CompositionPipeline, CompositorMsg, CompositorProxy, CompositorReceiver, ConstellationMsg,
    ForwardedToCompositorMsg, SendableFrameTree,
};
use crossbeam_channel::Sender;
use ipc_channel::ipc;
use log::{debug, error, trace, warn};
use servo::base::id::{PipelineId, TopLevelBrowsingContextId};
use servo::base::{Epoch, WebRenderEpochToU16};
use servo::embedder_traits::Cursor;
use servo::euclid::{Point2D, Scale, Transform3D, Vector2D};
use servo::profile_traits::time::{self as profile_time, profile, ProfilerCategory};
use servo::script_traits::CompositorEvent::{
    MouseButtonEvent, MouseMoveEvent, TouchEvent, WheelEvent,
};
use servo::script_traits::{
    AnimationState, AnimationTickType, ConstellationControlMsg, MouseButton, MouseEventType,
    ScrollState, TouchEventType, TouchId, WheelDelta, WindowSizeData, WindowSizeType,
};
use servo::servo_geometry::DeviceIndependentPixel;
use servo::style_traits::{CSSPixel, DevicePixel, PinchZoomFactor};
use servo::webrender_traits::display_list::{HitTestInfo, ScrollTree};
use servo::webrender_traits::{
    CanvasToCompositorMsg, CompositorHitTestResult, FontToCompositorMsg, ImageUpdate,
    NetToCompositorMsg, RenderingContext, ScriptToCompositorMsg, SerializedImageUpdate,
    UntrustedNodeAddress,
};
use webrender::api::units::{
    DeviceIntPoint, DeviceIntRect, DeviceIntSize, DevicePoint, LayoutPoint, LayoutRect, LayoutSize,
    LayoutVector2D, WorldPoint,
};
use webrender::api::{
    BuiltDisplayList, DirtyRect, DisplayListPayload, DocumentId, Epoch as WebRenderEpoch,
    ExternalScrollId, FontInstanceOptions, HitTestFlags, PipelineId as WebRenderPipelineId,
    PropertyBinding, ReferenceFrameKind, RenderReasons, SampledScrollOffset, ScrollLocation,
    SpaceAndClipInfo, SpatialId, SpatialTreeItemKey, TransformStyle,
};
use webrender::{RenderApi, Transaction};

use crate::touch::{TouchAction, TouchHandler};
use crate::webview::WebView;
use crate::window::Window;

/// Data used to construct a compositor.
pub struct InitialCompositorState {
    /// A channel to the compositor.
    pub sender: CompositorProxy,
    /// A port on which messages inbound to the compositor can be received.
    pub receiver: CompositorReceiver,
    /// A channel to the constellation.
    pub constellation_chan: Sender<ConstellationMsg>,
    /// A channel to the time profiler thread.
    pub time_profiler_chan: profile_traits::time::ProfilerChan,
    /// A channel to the memory profiler thread.
    pub mem_profiler_chan: profile_traits::mem::ProfilerChan,
    /// Instance of webrender API
    pub webrender: webrender::Renderer,
    pub webrender_document: DocumentId,
    pub webrender_api: RenderApi,
    pub rendering_context: RenderingContext,
    pub webrender_gl: Rc<dyn servo::gl::Gl>,
    pub webxr_main_thread: webxr::MainThreadRegistry,
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

// Default viewport constraints
const MAX_ZOOM: f32 = 8.0;
const MIN_ZOOM: f32 = 0.1;

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

/// NB: Never block on the constellation, because sometimes the constellation blocks on us.
pub struct IOCompositor {
    /// Size of current viewport that Compositor is handling.
    viewport: DeviceIntSize,

    /// The port on which we receive messages.
    port: CompositorReceiver,

    /// Tracks details about each active pipeline that the compositor knows about.
    pipeline_details: HashMap<PipelineId, PipelineDetails>,

    /// The pixel density of the display.
    scale_factor: Scale<f32, DeviceIndependentPixel, DevicePixel>,

    /// "Mobile-style" zoom that does not reflow the page.
    viewport_zoom: PinchZoomFactor,

    /// Viewport zoom constraints provided by @viewport.
    min_viewport_zoom: Option<PinchZoomFactor>,
    max_viewport_zoom: Option<PinchZoomFactor>,

    /// "Desktop-style" zoom that resizes the viewport to fit the window.
    page_zoom: Scale<f32, CSSPixel, DeviceIndependentPixel>,

    /// Tracks whether we should composite this frame.
    composition_request: CompositionRequest,

    /// Tracks whether we are in the process of shutting down, or have shut down and should close
    /// the compositor.
    pub shutdown_state: ShutdownState,

    /// Tracks whether the zoom action has happened recently.
    zoom_action: bool,

    /// The time of the last zoom action has started.
    zoom_time: f64,

    /// The current frame tree ID (used to reject old paint buffers)
    frame_tree_id: FrameTreeId,

    /// The channel on which messages can be sent to the constellation.
    pub constellation_chan: Sender<ConstellationMsg>,

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
    webrender: webrender::Renderer,

    /// The active webrender document.
    webrender_document: DocumentId,

    /// The webrender interface, if enabled.
    webrender_api: RenderApi,

    /// The surfman instance that webrender targets
    rendering_context: RenderingContext,

    /// The GL bindings for webrender
    webrender_gl: Rc<dyn servo::gl::Gl>,

    /// Some XR devices want to run on the main thread.
    pub webxr_main_thread: webxr::MainThreadRegistry,

    /// Map of the pending paint metrics per Layout.
    /// The Layout for each specific pipeline expects the compositor to
    /// paint frames with specific given IDs (epoch). Once the compositor paints
    /// these frames, it records the paint time for each of them and sends the
    /// metric to the corresponding Layout.
    pending_paint_metrics: HashMap<PipelineId, Epoch>,

    /// Current mouse cursor.
    cursor: Cursor,

    /// Current cursor position.
    cursor_pos: DevicePoint,

    /// True to exit after page load ('-x').
    exit_after_load: bool,

    /// True to translate mouse input into touch events.
    convert_mouse_to_touch: bool,

    /// The number of frames pending to receive from WebRender.
    pending_frames: usize,

    /// Waiting for external code to call present.
    waiting_on_present: bool,

    /// The [`Instant`] of the last animation tick, used to avoid flooding the Constellation and
    /// ScriptThread with a deluge of animation ticks.
    last_animation_tick: Instant,

    /// Whether the application is currently animating.
    /// Typically, when animations are active, the window
    /// will want to avoid blocking on UI events, and just
    /// run the event loop at the vsync interval.
    pub is_animating: bool,

    /// The order to paint webviews in, top most webview should be the last element.
    painting_order: Vec<WebView>,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShutdownState {
    NotShuttingDown,
    ShuttingDown,
    FinishedShuttingDown,
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
    pub fn new(
        viewport: DeviceIntSize,
        scale_factor: Scale<f32, DeviceIndependentPixel, DevicePixel>,
        state: InitialCompositorState,
        exit_after_load: bool,
        convert_mouse_to_touch: bool,
    ) -> Self {
        let compositor = IOCompositor {
            viewport,
            port: state.receiver,
            pipeline_details: HashMap::new(),
            scale_factor,
            composition_request: CompositionRequest::NoCompositingNecessary,
            touch_handler: TouchHandler::new(),
            pending_scroll_zoom_events: Vec::new(),
            shutdown_state: ShutdownState::NotShuttingDown,
            page_zoom: Scale::new(1.0),
            viewport_zoom: PinchZoomFactor::new(1.0),
            min_viewport_zoom: Some(PinchZoomFactor::new(1.0)),
            max_viewport_zoom: None,
            zoom_action: false,
            zoom_time: 0f64,
            frame_tree_id: FrameTreeId(0),
            constellation_chan: state.constellation_chan,
            time_profiler_chan: state.time_profiler_chan,
            ready_to_save_state: ReadyState::Unknown,
            webrender: state.webrender,
            webrender_document: state.webrender_document,
            webrender_api: state.webrender_api,
            rendering_context: state.rendering_context,
            webrender_gl: state.webrender_gl,
            webxr_main_thread: state.webxr_main_thread,
            pending_paint_metrics: HashMap::new(),
            cursor: Cursor::None,
            cursor_pos: DevicePoint::new(0.0, 0.0),
            exit_after_load,
            convert_mouse_to_touch,
            pending_frames: 0,
            waiting_on_present: false,
            last_animation_tick: Instant::now(),
            is_animating: false,
            painting_order: vec![],
        };

        // Make sure the GL state is OK
        compositor.assert_gl_framebuffer_complete();
        compositor
    }

    pub fn deinit(self) {
        if let Err(err) = self.rendering_context.make_gl_context_current() {
            warn!("Failed to make GL context current: {:?}", err);
        }
        self.webrender.deinit();
    }

    fn update_cursor(&mut self, result: CompositorHitTestResult) {
        let cursor = match result.cursor {
            Some(cursor) if cursor != self.cursor => cursor,
            _ => return,
        };

        self.cursor = cursor;
        let msg = ConstellationMsg::SetCursor(cursor);
        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending event to constellation failed ({:?}).", e);
        }
    }

    pub fn maybe_start_shutting_down(&mut self) {
        if self.shutdown_state == ShutdownState::NotShuttingDown {
            debug!("Shutting down the constellation for WindowEvent::Quit");
            self.start_shutting_down();
        }
    }

    fn start_shutting_down(&mut self) {
        debug!("Compositor sending Exit message to Constellation");
        if let Err(e) = self.constellation_chan.send(ConstellationMsg::Exit) {
            warn!("Sending exit message to constellation failed ({:?}).", e);
        }

        self.shutdown_state = ShutdownState::ShuttingDown;
    }

    fn finish_shutting_down(&mut self) {
        debug!("Compositor received message that constellation shutdown is complete");

        // Drain compositor port, sometimes messages contain channels that are blocking
        // another thread from finishing (i.e. SetFrameTree).
        while self.port.try_recv_compositor_msg().is_some() {}

        // Tell the profiler, memory profiler, and scrolling timer to shut down.
        if let Ok((sender, receiver)) = ipc::channel() {
            self.time_profiler_chan
                .send(profile_time::ProfilerMsg::Exit(sender));
            let _ = receiver.recv();
        }

        self.shutdown_state = ShutdownState::FinishedShuttingDown;
    }

    /// The underlying native surface can be lost during servo's lifetime.
    /// On Android, for example, this happens when the app is sent to background.
    /// We need to unbind the surface so that we don't try to use it again.
    pub fn invalidate_native_surface(&mut self) {
        debug!("Invalidating native surface in compositor");
        if let Err(e) = self.rendering_context.unbind_native_surface_from_context() {
            warn!("Unbinding native surface from context failed ({:?})", e);
        }
    }

    /// On Android, this function will be called when the app moves to foreground
    /// and the system creates a new native surface that needs to bound to the current
    /// context.
    #[allow(unsafe_code)]
    #[allow(clippy::not_unsafe_ptr_arg_deref)] // It has an unsafe block inside
    pub fn replace_native_surface(&mut self, native_widget: *mut c_void, coords: DeviceIntSize) {
        debug!("Replacing native surface in compositor: {native_widget:?}");
        let connection = self.rendering_context.connection();
        let native_widget =
            unsafe { connection.create_native_widget_from_ptr(native_widget, coords.to_untyped()) };
        if let Err(e) = self
            .rendering_context
            .bind_native_surface_to_context(native_widget)
        {
            warn!("Binding native surface to context failed ({:?})", e);
        }
    }

    fn handle_browser_message(&mut self, msg: CompositorMsg, window: &mut Window) -> bool {
        match self.shutdown_state {
            ShutdownState::NotShuttingDown => {}
            ShutdownState::ShuttingDown => {
                return self.handle_browser_message_while_shutting_down(msg)
            }
            ShutdownState::FinishedShuttingDown => {
                error!("compositor shouldn't be handling messages after shutting down");
                return false;
            }
        }

        match msg {
            CompositorMsg::ShutdownComplete => {
                warn!("Received `ShutdownComplete` while not shutting down.");
                self.finish_shutting_down();
                return false;
            }

            CompositorMsg::ChangeRunningAnimationsState(pipeline_id, animation_state) => {
                self.change_running_animations_state(pipeline_id, animation_state);
            }

            CompositorMsg::CreateOrUpdateWebView(frame_tree) => {
                self.set_frame_tree_for_webview(&frame_tree, window);
                self.send_scroll_positions_to_layout_for_pipeline(&frame_tree.pipeline.id);
            }

            CompositorMsg::RemoveWebView(top_level_browsing_context_id) => {
                self.remove_webview(top_level_browsing_context_id, window);
            }

            CompositorMsg::MoveResizeWebView(_webview_id, _rect) => {
                // TODO Remove this variant since it's no longer used.
                // self.move_resize_webview(webview_id, rect);
            }

            CompositorMsg::ShowWebView(_webview_id, _hide_others) => {
                // TODO Remove this variant since it's no longer used.
            }

            CompositorMsg::HideWebView(_webview_id) => {
                // TODO Remove this variant since it's no longer used.
            }

            CompositorMsg::RaiseWebViewToTop(_webview_id, _hide_others) => {
                // TODO Remove this variant since it's no longer used.
            }

            CompositorMsg::TouchEventProcessed(result) => {
                self.touch_handler.on_event_processed(result);
            }

            CompositorMsg::CreatePng(_page_rect, reply) => {
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

            CompositorMsg::SetThrottled(pipeline_id, throttled) => {
                self.pipeline_details(pipeline_id).throttled = throttled;
                self.process_animations(true);
            }

            CompositorMsg::PipelineExited(pipeline_id, sender) => {
                debug!("Compositor got pipeline exited: {:?}", pipeline_id);
                self.remove_pipeline_root_layer(pipeline_id);
                let _ = sender.send(());
            }

            CompositorMsg::NewWebRenderFrameReady(recomposite_needed) => {
                self.pending_frames -= 1;

                if recomposite_needed {
                    if let Some(result) = self.hit_test_at_point(self.cursor_pos) {
                        self.update_cursor(result);
                    }
                }

                if recomposite_needed || self.animation_callbacks_active() {
                    self.composite_if_necessary(CompositingReason::NewWebRenderFrame)
                }
            }

            CompositorMsg::LoadComplete(_) => {
                // If we're painting in headless mode, schedule a recomposite.
                if self.exit_after_load {
                    self.composite_if_necessary(CompositingReason::Headless);
                }
            }

            CompositorMsg::WebDriverMouseButtonEvent(mouse_event_type, mouse_button, x, y) => {
                let dppx = self.device_pixels_per_page_pixel();
                let point = dppx.transform_point(Point2D::new(x, y));
                self.on_mouse_window_event_class(match mouse_event_type {
                    MouseEventType::Click => MouseWindowEvent::Click(mouse_button, point),
                    MouseEventType::MouseDown => MouseWindowEvent::MouseDown(mouse_button, point),
                    MouseEventType::MouseUp => MouseWindowEvent::MouseUp(mouse_button, point),
                });
            }

            CompositorMsg::WebDriverMouseMoveEvent(x, y) => {
                let dppx = self.device_pixels_per_page_pixel();
                let point = dppx.transform_point(Point2D::new(x, y));
                self.on_mouse_window_move_event_class(DevicePoint::new(point.x, point.y));
            }

            CompositorMsg::PendingPaintMetric(pipeline_id, epoch) => {
                self.pending_paint_metrics.insert(pipeline_id, epoch);
            }

            CompositorMsg::GetClientWindow(req) => {
                // TODO get real size
                if let Err(e) = req.send((self.viewport, Point2D::new(0, 0))) {
                    warn!("Sending response to get client window failed ({:?}).", e);
                }
            }

            CompositorMsg::GetScreenSize(req) => {
                // TODO get real size
                if let Err(e) = req.send(self.viewport) {
                    warn!("Sending response to get screen size failed ({:?}).", e);
                }
            }

            CompositorMsg::GetScreenAvailSize(req) => {
                // TODO get real size
                if let Err(e) = req.send(self.viewport) {
                    warn!(
                        "Sending response to get screen avail size failed ({:?}).",
                        e
                    );
                }
            }

            CompositorMsg::Forwarded(msg) => {
                self.handle_webrender_message(msg);
            }
        }

        true
    }

    /// Accept messages from content processes that need to be relayed to the WebRender
    /// instance in the parent process.
    fn handle_webrender_message(&mut self, msg: ForwardedToCompositorMsg) {
        match msg {
            ForwardedToCompositorMsg::Layout(ScriptToCompositorMsg::SendInitialTransaction(
                pipeline,
            )) => {
                let mut txn = Transaction::new();
                txn.set_display_list(WebRenderEpoch(0), (pipeline, Default::default()));
                self.generate_frame(&mut txn, RenderReasons::SCENE);
                self.webrender_api
                    .send_transaction(self.webrender_document, txn);
            }

            ForwardedToCompositorMsg::Layout(ScriptToCompositorMsg::SendScrollNode(
                pipeline_id,
                point,
                external_scroll_id,
            )) => {
                let pipeline_id = pipeline_id.into();
                let pipeline_details = match self.pipeline_details.get_mut(&pipeline_id) {
                    Some(details) => details,
                    None => return,
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
                    return;
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

            ForwardedToCompositorMsg::Layout(ScriptToCompositorMsg::SendDisplayList {
                display_list_info,
                display_list_descriptor,
                display_list_receiver,
            }) => {
                // This must match the order from the sender, currently in `shared/script/lib.rs`.
                let items_data = match display_list_receiver.recv() {
                    Ok(display_list_data) => display_list_data,
                    Err(error) => {
                        return warn!(
                            "Could not receive WebRender display list items data: {error}"
                        )
                    }
                };
                let cache_data = match display_list_receiver.recv() {
                    Ok(display_list_data) => display_list_data,
                    Err(error) => {
                        return warn!(
                            "Could not receive WebRender display list cache data: {error}"
                        )
                    }
                };
                let spatial_tree = match display_list_receiver.recv() {
                    Ok(display_list_data) => display_list_data,
                    Err(error) => {
                        return warn!(
                            "Could not receive WebRender display list spatial tree: {error}."
                        )
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

                let mut transaction = Transaction::new();
                transaction
                    .set_display_list(display_list_info.epoch, (pipeline_id, built_display_list));
                self.update_transaction_with_all_scroll_offsets(&mut transaction);
                self.generate_frame(&mut transaction, RenderReasons::SCENE);
                self.webrender_api
                    .send_transaction(self.webrender_document, transaction);
            }

            ForwardedToCompositorMsg::Layout(ScriptToCompositorMsg::HitTest(
                pipeline,
                point,
                flags,
                sender,
            )) => {
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

            ForwardedToCompositorMsg::Layout(ScriptToCompositorMsg::GenerateImageKey(sender))
            | ForwardedToCompositorMsg::Net(NetToCompositorMsg::GenerateImageKey(sender)) => {
                let _ = sender.send(self.webrender_api.generate_image_key());
            }

            ForwardedToCompositorMsg::Layout(ScriptToCompositorMsg::UpdateImages(updates)) => {
                let mut txn = Transaction::new();
                for update in updates {
                    match update {
                        SerializedImageUpdate::AddImage(key, desc, data) => {
                            match data.to_image_data() {
                                Ok(data) => txn.add_image(key, desc, data, None),
                                Err(e) => warn!("error when sending image data: {:?}", e),
                            }
                        }
                        SerializedImageUpdate::DeleteImage(key) => txn.delete_image(key),
                        SerializedImageUpdate::UpdateImage(key, desc, data) => {
                            match data.to_image_data() {
                                Ok(data) => txn.update_image(key, desc, data, &DirtyRect::All),
                                Err(e) => warn!("error when sending image data: {:?}", e),
                            }
                        }
                    }
                }
                self.webrender_api
                    .send_transaction(self.webrender_document, txn);
            }

            ForwardedToCompositorMsg::Layout(ScriptToCompositorMsg::RemoveFonts(
                keys,
                instance_keys,
            )) => {
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

            ForwardedToCompositorMsg::Net(NetToCompositorMsg::AddImage(key, desc, data)) => {
                let mut txn = Transaction::new();
                txn.add_image(key, desc, data, None);
                self.webrender_api
                    .send_transaction(self.webrender_document, txn);
            }

            ForwardedToCompositorMsg::Font(FontToCompositorMsg::AddFontInstance(
                font_key,
                size,
                flags,
                sender,
            )) => {
                let key = self.webrender_api.generate_font_instance_key();
                let mut transaction = Transaction::new();

                let font_instance_options = FontInstanceOptions {
                    flags,
                    ..Default::default()
                };
                transaction.add_font_instance(
                    key,
                    font_key,
                    size,
                    Some(font_instance_options),
                    None,
                    Vec::new(),
                );

                self.webrender_api
                    .send_transaction(self.webrender_document, transaction);
                let _ = sender.send(key);
            }

            ForwardedToCompositorMsg::Font(FontToCompositorMsg::AddFont(
                key_sender,
                index,
                bytes_receiver,
            )) => {
                let font_key = self.webrender_api.generate_font_key();
                let mut transaction = Transaction::new();
                let bytes = bytes_receiver.recv().unwrap_or_default();
                transaction.add_raw_font(font_key, bytes, index);
                self.webrender_api
                    .send_transaction(self.webrender_document, transaction);
                let _ = key_sender.send(font_key);
            }

            ForwardedToCompositorMsg::Font(FontToCompositorMsg::AddSystemFont(
                key_sender,
                native_handle,
            )) => {
                let font_key = self.webrender_api.generate_font_key();
                let mut transaction = Transaction::new();
                transaction.add_native_font(font_key, native_handle);
                self.webrender_api
                    .send_transaction(self.webrender_document, transaction);
                let _ = key_sender.send(font_key);
            }

            ForwardedToCompositorMsg::Canvas(CanvasToCompositorMsg::GenerateKey(sender)) => {
                let _ = sender.send(self.webrender_api.generate_image_key());
            }

            ForwardedToCompositorMsg::Canvas(CanvasToCompositorMsg::UpdateImages(updates)) => {
                let mut txn = Transaction::new();
                for update in updates {
                    match update {
                        ImageUpdate::AddImage(key, descriptor, data) => {
                            txn.add_image(key, descriptor, data, None)
                        }
                        ImageUpdate::UpdateImage(key, descriptor, data) => {
                            txn.update_image(key, descriptor, data, &DirtyRect::All)
                        }
                        ImageUpdate::DeleteImage(key) => txn.delete_image(key),
                    }
                }
                self.webrender_api
                    .send_transaction(self.webrender_document, txn);
            }
        }
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
            CompositorMsg::ShutdownComplete => {
                self.finish_shutting_down();
                return false;
            }
            CompositorMsg::PipelineExited(pipeline_id, sender) => {
                debug!("Compositor got pipeline exited: {:?}", pipeline_id);
                self.remove_pipeline_root_layer(pipeline_id);
                let _ = sender.send(());
            }
            CompositorMsg::Forwarded(ForwardedToCompositorMsg::Font(
                FontToCompositorMsg::AddFontInstance(_, _, _, sender),
            )) => {
                let _ = sender.send(self.webrender_api.generate_font_instance_key());
            }
            CompositorMsg::Forwarded(ForwardedToCompositorMsg::Font(
                FontToCompositorMsg::AddFont(sender, _, _),
            )) => {
                let _ = sender.send(self.webrender_api.generate_font_key());
            }
            CompositorMsg::Forwarded(ForwardedToCompositorMsg::Canvas(
                CanvasToCompositorMsg::GenerateKey(sender),
            )) => {
                let _ = sender.send(self.webrender_api.generate_image_key());
            }
            CompositorMsg::GetClientWindow(sender) => {
                if let Err(e) = sender.send((self.viewport, Point2D::new(0, 0))) {
                    warn!("Sending response to get client window failed ({:?}).", e);
                }
            }
            CompositorMsg::GetScreenSize(sender) => {
                if let Err(e) = sender.send(self.viewport) {
                    warn!("Sending response to get screen size failed ({:?}).", e);
                }
            }
            CompositorMsg::GetScreenAvailSize(sender) => {
                if let Err(e) = sender.send(self.viewport) {
                    warn!(
                        "Sending response to get screen avail size failed ({:?}).",
                        e
                    );
                }
            }
            CompositorMsg::NewWebRenderFrameReady(_) => {
                // Subtract from the number of pending frames, but do not do any compositing.
                self.pending_frames -= 1;
            }
            CompositorMsg::PendingPaintMetric(pipeline_id, epoch) => {
                self.pending_paint_metrics.insert(pipeline_id, epoch);
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
        transaction.generate_frame(0, reason);
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

    pub fn pipeline(&self, pipeline_id: PipelineId) -> Option<&CompositionPipeline> {
        match self.pipeline_details.get(&pipeline_id) {
            Some(details) => details.pipeline.as_ref(),
            None => {
                warn!(
                    "Compositor layer has an unknown pipeline ({:?}).",
                    pipeline_id
                );
                None
            }
        }
    }

    /// Set the root pipeline for our WebRender scene to a display list that consists of an iframe
    /// for each visible top-level browsing context, applying a transformation on the root for
    /// pinch zoom, page zoom, and HiDPI scaling.
    fn send_root_pipeline_display_list(&mut self) {
        let mut transaction = Transaction::new();
        self.send_root_pipeline_display_list_in_transaction(&mut transaction);
        self.generate_frame(&mut transaction, RenderReasons::SCENE);
        self.webrender_api
            .send_transaction(self.webrender_document, transaction);
    }

    /// Set the root pipeline for our WebRender scene to a display list that consists of an iframe
    /// for each visible top-level browsing context, applying a transformation on the root for
    /// pinch zoom, page zoom, and HiDPI scaling.
    fn send_root_pipeline_display_list_in_transaction(&self, transaction: &mut Transaction) {
        // Every display list needs a pipeline, but we'd like to choose one that is unlikely
        // to conflict with our content pipelines, which start at (1, 1). (0, 0) is WebRender's
        // dummy pipeline, so we choose (0, 1).
        let root_pipeline = WebRenderPipelineId(0, 1);
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

        let scaled_viewport_size = self.viewport.to_f32() / zoom_factor;
        let scaled_viewport_size = LayoutSize::from_untyped(scaled_viewport_size.to_untyped());
        let scaled_viewport_rect =
            LayoutRect::from_origin_and_size(LayoutPoint::zero(), scaled_viewport_size);

        let root_clip_id = builder.define_clip_rect(zoom_reference_frame, scaled_viewport_rect);
        let clip_chain_id = builder.define_clip_chain(None, [root_clip_id]);
        for webview in &self.painting_order {
            if let Some(pipeline_id) = webview.pipeline_id {
                let scaled_webview_rect = webview.rect.to_f32() / zoom_factor;
                builder.push_iframe(
                    LayoutRect::from_untyped(&scaled_webview_rect.to_untyped()),
                    LayoutRect::from_untyped(&scaled_webview_rect.to_untyped()),
                    &SpaceAndClipInfo {
                        spatial_id: zoom_reference_frame,
                        clip_chain_id,
                    },
                    pipeline_id.into(),
                    true,
                );
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

    fn set_frame_tree_for_webview(&mut self, frame_tree: &SendableFrameTree, window: &mut Window) {
        debug!("{}: Setting frame tree for webview", frame_tree.pipeline.id);

        window.set_webview(
            frame_tree.pipeline.top_level_browsing_context_id,
            frame_tree.pipeline.id,
            self,
        );

        self.send_root_pipeline_display_list();
        self.create_or_update_pipeline_details_with_frame_tree(frame_tree, None);
        self.reset_scroll_tree_for_unattached_pipelines(frame_tree);

        self.frame_tree_id.next();
    }

    fn remove_webview(
        &mut self,
        top_level_browsing_context_id: TopLevelBrowsingContextId,
        window: &mut Window,
    ) {
        debug!("{}: Removing", top_level_browsing_context_id);
        let Some(webview) = window.remove_webview(top_level_browsing_context_id, self) else {
            warn!("{top_level_browsing_context_id}: Removing unknown webview");
            return;
        };

        self.send_root_pipeline_display_list();
        if let Some(pipeline_id) = webview.pipeline_id {
            self.remove_pipeline_details_recursively(pipeline_id);
        }

        self.frame_tree_id.next();
    }

    pub fn webview_resized(&mut self, webview_id: TopLevelBrowsingContextId, rect: DeviceIntRect) {
        self.send_window_size_message_for_top_level_browser_context(rect, webview_id);
        self.send_root_pipeline_display_list();
    }

    fn send_window_size_message_for_top_level_browser_context(
        &self,
        rect: DeviceIntRect,
        top_level_browsing_context_id: TopLevelBrowsingContextId,
    ) {
        // The device pixel ratio used by the style system should include the scale from page pixels
        // to device pixels, but not including any pinch zoom.
        let device_pixel_ratio = self.device_pixels_per_page_pixel_not_including_page_zoom();
        let initial_viewport = rect.size().to_f32() / device_pixel_ratio;
        let msg = ConstellationMsg::WindowSize(
            top_level_browsing_context_id,
            WindowSizeData {
                device_pixel_ratio,
                initial_viewport,
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

    pub fn on_resize_window_event(&mut self, new_viewport: DeviceIntSize) -> bool {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return false;
        }

        let _ = self.rendering_context.resize(new_viewport.to_untyped());
        self.viewport = new_viewport;
        let mut transaction = Transaction::new();
        transaction.set_document_view(DeviceIntRect::from_size(self.viewport));
        self.webrender_api
            .send_transaction(self.webrender_document, transaction);
        self.update_after_zoom_or_hidpi_change();
        self.composite_if_necessary(CompositingReason::Resize);
        true
    }

    pub fn on_mouse_window_event_class(&mut self, mouse_window_event: MouseWindowEvent) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        if self.convert_mouse_to_touch {
            match mouse_window_event {
                MouseWindowEvent::Click(_, _) => {}
                MouseWindowEvent::MouseDown(_, p) => self.on_touch_down(TouchId(0), p),
                MouseWindowEvent::MouseUp(_, p) => self.on_touch_up(TouchId(0), p),
            }
            return;
        }

        self.dispatch_mouse_window_event_class(mouse_window_event);
    }

    fn dispatch_mouse_window_event_class(&mut self, mouse_window_event: MouseWindowEvent) {
        let point = match mouse_window_event {
            MouseWindowEvent::Click(_, p) => p,
            MouseWindowEvent::MouseDown(_, p) => p,
            MouseWindowEvent::MouseUp(_, p) => p,
        };

        let Some(result) = self.hit_test_at_point(point) else {
            // TODO: Notify embedder that the event failed to hit test to any webview.
            // TODO: Also notify embedder if an event hits a webview but isn’t consumed?
            return;
        };

        let (button, event_type) = match mouse_window_event {
            MouseWindowEvent::Click(button, _) => (button, MouseEventType::Click),
            MouseWindowEvent::MouseDown(button, _) => (button, MouseEventType::MouseDown),
            MouseWindowEvent::MouseUp(button, _) => (button, MouseEventType::MouseUp),
        };

        let event_to_send = MouseButtonEvent(
            event_type,
            button,
            result.point_in_viewport.to_untyped(),
            Some(result.node.into()),
            Some(result.point_relative_to_item),
            button as u16,
        );

        let msg = ConstellationMsg::ForwardEvent(result.pipeline_id, event_to_send);
        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending event to constellation failed ({:?}).", e);
        }
    }

    fn hit_test_at_point(&self, point: DevicePoint) -> Option<CompositorHitTestResult> {
        return self
            .hit_test_at_point_with_flags_and_pipeline(point, HitTestFlags::empty(), None)
            .first()
            .cloned();
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

    pub fn on_mouse_window_move_event_class(&mut self, cursor: DevicePoint) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        if self.convert_mouse_to_touch {
            self.on_touch_move(TouchId(0), cursor);
            return;
        }

        self.dispatch_mouse_window_move_event_class(cursor);
    }

    fn dispatch_mouse_window_move_event_class(&mut self, cursor: DevicePoint) {
        let result = match self.hit_test_at_point(cursor) {
            Some(result) => result,
            None => return,
        };

        self.cursor_pos = cursor;
        let event = MouseMoveEvent(result.point_in_viewport, Some(result.node.into()), 0);
        let msg = ConstellationMsg::ForwardEvent(result.pipeline_id, event);
        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending event to constellation failed ({:?}).", e);
        }
        self.update_cursor(result);
    }

    fn send_touch_event(
        &self,
        event_type: TouchEventType,
        identifier: TouchId,
        point: DevicePoint,
    ) {
        if let Some(result) = self.hit_test_at_point(point) {
            let event = TouchEvent(
                event_type,
                identifier,
                result.point_in_viewport,
                Some(result.node.into()),
            );
            let msg = ConstellationMsg::ForwardEvent(result.pipeline_id, event);
            if let Err(e) = self.constellation_chan.send(msg) {
                warn!("Sending event to constellation failed ({:?}).", e);
            }
        }
    }

    pub fn send_wheel_event(&mut self, delta: WheelDelta, point: DevicePoint) {
        if let Some(result) = self.hit_test_at_point(point) {
            let event = WheelEvent(delta, result.point_in_viewport, Some(result.node.into()));
            let msg = ConstellationMsg::ForwardEvent(result.pipeline_id, event);
            if let Err(e) = self.constellation_chan.send(msg) {
                warn!("Sending event to constellation failed ({:?}).", e);
            }
        }
    }

    pub fn on_touch_event(
        &mut self,
        event_type: TouchEventType,
        identifier: TouchId,
        location: DevicePoint,
    ) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        match event_type {
            TouchEventType::Down => self.on_touch_down(identifier, location),
            TouchEventType::Move => self.on_touch_move(identifier, location),
            TouchEventType::Up => self.on_touch_up(identifier, location),
            TouchEventType::Cancel => self.on_touch_cancel(identifier, location),
        }
    }

    fn on_touch_down(&mut self, identifier: TouchId, point: DevicePoint) {
        self.touch_handler.on_touch_down(identifier, point);
        self.send_touch_event(TouchEventType::Down, identifier, point);
    }

    fn on_touch_move(&mut self, identifier: TouchId, point: DevicePoint) {
        match self.touch_handler.on_touch_move(identifier, point) {
            TouchAction::Scroll(delta) => self.on_scroll_window_event(
                ScrollLocation::Delta(LayoutVector2D::from_untyped(delta.to_untyped())),
                point.cast(),
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
            TouchAction::DispatchEvent => {
                self.send_touch_event(TouchEventType::Move, identifier, point);
            }
            _ => {}
        }
    }

    fn on_touch_up(&mut self, identifier: TouchId, point: DevicePoint) {
        self.send_touch_event(TouchEventType::Up, identifier, point);

        if let TouchAction::Click = self.touch_handler.on_touch_up(identifier, point) {
            self.simulate_mouse_click(point);
        }
    }

    fn on_touch_cancel(&mut self, identifier: TouchId, point: DevicePoint) {
        // Send the event to script.
        self.touch_handler.on_touch_cancel(identifier, point);
        self.send_touch_event(TouchEventType::Cancel, identifier, point);
    }

    /// <http://w3c.github.io/touch-events/#mouse-events>
    fn simulate_mouse_click(&mut self, p: DevicePoint) {
        let button = MouseButton::Left;
        self.dispatch_mouse_window_move_event_class(p);
        self.dispatch_mouse_window_event_class(MouseWindowEvent::MouseDown(button, p));
        self.dispatch_mouse_window_event_class(MouseWindowEvent::MouseUp(button, p));
        self.dispatch_mouse_window_event_class(MouseWindowEvent::Click(button, p));
    }

    pub fn on_wheel_event(&mut self, delta: WheelDelta, p: DevicePoint) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        self.send_wheel_event(delta, p);
    }

    pub fn on_scroll_event(
        &mut self,
        scroll_location: ScrollLocation,
        cursor: DeviceIntPoint,
        phase: TouchEventType,
    ) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        match phase {
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

    fn process_pending_scroll_events(&mut self) {
        // Batch up all scroll events into one, or else we'll do way too much painting.
        let mut combined_scroll_event: Option<ScrollEvent> = None;
        let mut combined_magnification = 1.0;
        for scroll_event in self.pending_scroll_zoom_events.drain(..) {
            match scroll_event {
                ScrollZoomEvent::PinchZoom(magnification) => {
                    combined_magnification *= magnification
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

        let zoom_changed =
            self.set_pinch_zoom_level(self.pinch_zoom_level().get() * combined_magnification);
        let scroll_result = combined_scroll_event.and_then(|combined_event| {
            self.scroll_node_at_device_point(
                combined_event.cursor.to_f32(),
                combined_event.scroll_location,
            )
        });
        if !zoom_changed && scroll_result.is_none() {
            return;
        }

        let mut transaction = Transaction::new();
        if zoom_changed {
            self.send_root_pipeline_display_list_in_transaction(&mut transaction);
        }

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
                    .get_mut(&pipeline_id)?
                    .scroll_tree
                    .scroll_node_or_ancestor(&scroll_tree_node, scroll_location);
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
        if pipeline_ids.is_empty() && !self.webxr_main_thread.running() {
            self.is_animating = false;
        } else {
            self.is_animating = true;
        };
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

        let msg = ConstellationMsg::TickAnimation(pipeline_id, tick_type);
        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending tick to constellation failed ({:?}).", e);
        }
    }

    fn scale_factor(&self) -> Scale<f32, DeviceIndependentPixel, DevicePixel> {
        self.scale_factor
    }

    fn device_pixels_per_page_pixel(&self) -> Scale<f32, CSSPixel, DevicePixel> {
        self.device_pixels_per_page_pixel_not_including_page_zoom() * self.pinch_zoom_level()
    }

    fn device_pixels_per_page_pixel_not_including_page_zoom(
        &self,
    ) -> Scale<f32, CSSPixel, DevicePixel> {
        self.page_zoom * self.scale_factor()
    }

    pub fn on_zoom_reset_window_event(&mut self) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        self.page_zoom = Scale::new(1.0);
        self.update_after_zoom_or_hidpi_change();
    }

    pub fn on_zoom_window_event(&mut self, magnification: f32) {
        if self.shutdown_state != ShutdownState::NotShuttingDown {
            return;
        }

        self.page_zoom = Scale::new(
            (self.page_zoom.get() * magnification)
                .max(MIN_ZOOM)
                .min(MAX_ZOOM),
        );
        self.update_after_zoom_or_hidpi_change();
    }

    fn update_after_zoom_or_hidpi_change(&mut self) {
        for webview in &self.painting_order {
            self.send_window_size_message_for_top_level_browser_context(
                webview.rect,
                webview.webview_id(),
            );
        }

        // Update the root transform in WebRender to reflect the new zoom.
        self.send_root_pipeline_display_list();
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

        if let Some(pipeline) = details.pipeline.as_ref() {
            let message = ConstellationControlMsg::SetScrollStates(*pipeline_id, scroll_states);
            let _ = pipeline.script_chan.send(message);
        }
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
                        .current_epoch(self.webrender_document, id.into())
                    {
                        let epoch = Epoch(epoch);
                        pipeline_epochs.insert(*id, epoch);
                    }
                }

                // Pass the pipeline/epoch states to the constellation and check
                // if it's safe to output the image.
                let msg = ConstellationMsg::IsReadyToSaveImage(pipeline_epochs);
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

    pub fn composite(&mut self) {
        match self.composite_specific_target() {
            Ok(_) => {
                if self.exit_after_load {
                    println!("Shutting down the Constellation after generating an output file or exit flag specified");
                    self.start_shutting_down();
                }
            }
            Err(error) => {
                trace!("Unable to composite: {error:?}");
            }
        }
    }

    /// Composite to the given target if any, or the current target otherwise.
    /// Returns Ok if composition was performed or Err if it was not possible to composite for some
    /// reason. When the target is [CompositeTarget::SharedMemory], the image is read back from the
    /// GPU and returned as Ok(Some(png::Image)), otherwise we return Ok(None).
    fn composite_specific_target(&mut self) -> Result<(), UnableToComposite> {
        if self.waiting_on_present {
            debug!("tried to composite while waiting on present");
            return Err(UnableToComposite::NotReadyToPaintImage(
                NotReadyToPaint::WaitingOnConstellation,
            ));
        }

        if let Err(err) = self.rendering_context.make_gl_context_current() {
            warn!("Failed to make GL context current: {:?}", err);
        }
        self.assert_no_gl_error();

        self.webrender.update();

        let wait_for_stable_image = self.exit_after_load;

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

        // Bind the webrender framebuffer
        let framebuffer_object = self
            .rendering_context
            .context_surface_info()
            .unwrap_or(None)
            .map(|info| info.framebuffer_object)
            .unwrap_or(0);
        self.webrender_gl
            .bind_framebuffer(servo::gl::FRAMEBUFFER, framebuffer_object);
        self.assert_gl_framebuffer_complete();

        profile(
            ProfilerCategory::Compositing,
            None,
            self.time_profiler_chan.clone(),
            || {
                trace!("Compositing");
                // Paint the scene.
                // TODO(gw): Take notice of any errors the renderer returns!
                self.clear_background();
                self.webrender
                    // TODO to untyped?
                    .render(self.viewport, 0 /* buffer_age */)
                    .ok();
            },
        );

        // If there are pending paint metrics, we check if any of the painted epochs is one of the
        // ones that the paint metrics recorder is expecting. In that case, we get the current
        // time, inform layout about it and remove the pending metric from the list.
        if !self.pending_paint_metrics.is_empty() {
            let paint_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let mut to_remove = Vec::new();
            // For each pending paint metrics pipeline id
            for (id, pending_epoch) in &self.pending_paint_metrics {
                // we get the last painted frame id from webrender
                if let Some(WebRenderEpoch(epoch)) = self
                    .webrender
                    .current_epoch(self.webrender_document, id.into())
                {
                    // and check if it is the one layout is expecting,
                    let epoch = Epoch(epoch);
                    if *pending_epoch != epoch {
                        warn!(
                            "{}: paint metrics: pending {:?} should be {:?}",
                            id, pending_epoch, epoch
                        );
                        continue;
                    }
                    // in which case, we remove it from the list of pending metrics,
                    to_remove.push(*id);
                    if let Some(pipeline) = self.pipeline(*id) {
                        // and inform layout with the measured paint time.
                        if let Err(e) =
                            pipeline
                                .script_chan
                                .send(ConstellationControlMsg::SetEpochPaintTime(
                                    *id, epoch, paint_time,
                                ))
                        {
                            warn!("Sending RequestLayoutPaintMetric message to layout failed ({e:?}).");
                        }
                    }
                }
            }
            for id in to_remove.iter() {
                self.pending_paint_metrics.remove(id);
            }
        }

        // Notify embedder that servo is ready to present.
        // Embedder should call `present` to tell compositor to continue rendering.
        self.waiting_on_present = true;
        let webview_ids = self.painting_order.iter().map(|w| w.webview_id());
        let msg = ConstellationMsg::ReadyToPresent(webview_ids.collect());
        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending event to constellation failed ({:?}).", e);
        }

        self.composition_request = CompositionRequest::NoCompositingNecessary;

        self.process_animations(true);

        Ok(())
    }

    pub fn present(&mut self) {
        if let Err(err) = self.rendering_context.present() {
            warn!("Failed to present surface: {:?}", err);
        }
        self.waiting_on_present = false;
    }

    fn composite_if_necessary(&mut self, reason: CompositingReason) {
        trace!(
            "Will schedule a composite {reason:?}. Previously was {:?}",
            self.composition_request
        );
        self.composition_request = CompositionRequest::CompositeNow(reason)
    }

    fn clear_background(&self) {
        let gl = &self.webrender_gl;
        self.assert_gl_framebuffer_complete();

        // Set the viewport background based on prefs.
        let color = servo::config::pref!(shell.background_color.rgba);
        gl.clear_color(
            color[0] as f32,
            color[1] as f32,
            color[2] as f32,
            color[3] as f32,
        );

        let framebuffer_height = self.viewport.height;
        // Clear the viewport rect of each top-level browsing context.
        for webview in &self.painting_order {
            // Flip the rectangle in the framebuffer
            let origin = webview.rect.to_i32();
            let mut rect = origin.clone();
            let min_y = framebuffer_height - rect.max.y;
            let max_y = framebuffer_height - rect.min.y;
            rect.min.y = min_y;
            rect.max.y = max_y;

            gl.scissor(
                rect.min.x,
                rect.min.y,
                rect.size().width,
                rect.size().height,
            );
            gl.enable(servo::gl::SCISSOR_TEST);
            gl.clear(servo::gl::COLOR_BUFFER_BIT);
            gl.disable(servo::gl::SCISSOR_TEST);
        }

        self.assert_gl_framebuffer_complete();
    }

    #[track_caller]
    fn assert_no_gl_error(&self) {
        debug_assert_eq!(self.webrender_gl.get_error(), servo::gl::NO_ERROR);
    }

    #[track_caller]
    fn assert_gl_framebuffer_complete(&self) {
        debug_assert_eq!(
            (
                self.webrender_gl.get_error(),
                self.webrender_gl
                    .check_frame_buffer_status(servo::gl::FRAMEBUFFER)
            ),
            (servo::gl::NO_ERROR, servo::gl::FRAMEBUFFER_COMPLETE)
        );
    }

    pub fn receive_messages(&mut self, window: &mut Window) -> bool {
        // Check for new messages coming from the other threads in the system.
        let mut compositor_messages = vec![];
        let mut found_recomposite_msg = false;
        while let Some(msg) = self.port.try_recv_compositor_msg() {
            match msg {
                CompositorMsg::NewWebRenderFrameReady(_) if found_recomposite_msg => {
                    // Only take one of duplicate NewWebRendeFrameReady messages, but do subtract
                    // one frame from the pending frames.
                    self.pending_frames -= 1;
                }
                CompositorMsg::NewWebRenderFrameReady(_) => {
                    found_recomposite_msg = true;
                    compositor_messages.push(msg)
                }
                _ => compositor_messages.push(msg),
            }
        }
        for msg in compositor_messages {
            if !self.handle_browser_message(msg, window) {
                return false;
            }
        }
        true
    }

    pub fn perform_updates(&mut self) -> bool {
        if self.shutdown_state == ShutdownState::FinishedShuttingDown {
            return false;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as f64;
        // If a pinch-zoom happened recently, ask for tiles at the new resolution
        if self.zoom_action && now - self.zoom_time > 0.3 {
            self.zoom_action = false;
        }

        match self.composition_request {
            CompositionRequest::NoCompositingNecessary => {}
            CompositionRequest::CompositeNow(_) => self.composite(),
        }

        // Run the WebXR main thread
        self.webxr_main_thread.run_one_frame();

        // The WebXR thread may make a different context current
        let _ = self.rendering_context.make_gl_context_current();

        if !self.pending_scroll_zoom_events.is_empty() {
            self.process_pending_scroll_events()
        }
        self.shutdown_state != ShutdownState::FinishedShuttingDown
    }
    /// Repaints and recomposites synchronously. You must be careful when calling this, as if a
    /// paint is not scheduled the compositor will hang forever.
    ///
    /// This is used when resizing the window.
    pub fn repaint_synchronously(&mut self, window: &mut Window) {
        while self.shutdown_state != ShutdownState::ShuttingDown {
            let msg = self.port.recv_compositor_msg();
            let need_recomposite = matches!(msg, CompositorMsg::NewWebRenderFrameReady(_));
            let keep_going = self.handle_browser_message(msg, window);
            if need_recomposite {
                self.composite();
                break;
            }
            if !keep_going {
                break;
            }
        }
    }

    pub fn pinch_zoom_level(&self) -> Scale<f32, DevicePixel, DevicePixel> {
        Scale::new(self.viewport_zoom.get())
    }

    fn set_pinch_zoom_level(&mut self, mut zoom: f32) -> bool {
        if let Some(min) = self.min_viewport_zoom {
            zoom = f32::max(min.get(), zoom);
        }
        if let Some(max) = self.max_viewport_zoom {
            zoom = f32::min(max.get(), zoom);
        }

        let old_zoom = std::mem::replace(&mut self.viewport_zoom, PinchZoomFactor::new(zoom));
        old_zoom != self.viewport_zoom
    }

    pub fn toggle_webrender_debug(&mut self, option: WebRenderDebugOption) {
        let mut flags = self.webrender.get_debug_flags();
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
        self.webrender.set_debug_flags(flags);

        let mut txn = Transaction::new();
        self.generate_frame(&mut txn, RenderReasons::TESTING);
        self.webrender_api
            .send_transaction(self.webrender_document, txn);
    }

    pub fn set_painting_order(&mut self, painting_order: Vec<WebView>) {
        self.painting_order = painting_order;
    }
}

/// Various debug and profiling flags that WebRender supports.
#[derive(Clone)]
pub enum WebRenderDebugOption {
    Profiler,
    TextureCacheDebug,
    RenderTargetDebug,
}

#[derive(Clone)]
pub enum MouseWindowEvent {
    Click(MouseButton, DevicePoint),
    MouseDown(MouseButton, DevicePoint),
    MouseUp(MouseButton, DevicePoint),
}