use std::rc::Rc;

use servo::{
    compositing::{
        windowing::{EmbedderEvent, EmbedderMethods},
        CompositeTarget,
    },
    embedder_traits::{Cursor, EmbedderMsg, EventLoopWaker},
    servo_url::ServoUrl,
    Servo,
};
use winit::{
    event::Event,
    event_loop::EventLoopProxy,
    window::{CursorIcon, Window},
};

use crate::{prefs, resources, webview::WebView};

/// Status of Verso instance.
#[derive(Clone, Copy, Debug, Default)]
pub enum Status {
    /// Nothing happed to Verso at the moment.
    #[default]
    None,
    /// One of webviews is animating.
    Animating,
    /// Verso has shut down.
    Shutdown,
}

/// Main entry point of Verso browser.
pub struct Verso {
    // TODO Verso should have servo, (Verso) windows as fields.
    servo: Option<Servo<WebView>>,
    webview: Rc<WebView>,
    events: Vec<EmbedderEvent>,
    // TODO following fields should move to webvew
    status: Status,
}

impl Verso {
    /// Create an Verso instance from winit's window and event loop proxy.
    pub fn new(window: Window, proxy: EventLoopProxy<()>) -> Self {
        resources::init();
        prefs::init();

        let webview = Rc::new(WebView::new(window));
        let callback = Box::new(Embedder(proxy));
        let mut init_servo = Servo::new(
            callback,
            webview.clone(),
            Some(String::from(
                "Mozilla/5.0 (X11; Linux x86_64; rv:109.0) Gecko/20100101 Firefox/119.0",
            )),
            CompositeTarget::Fbo,
        );

        // let demo_path = std::env::current_dir().unwrap().join("demo.html");
        // let url = ServoUrl::from_file_path(demo_path.to_str().unwrap()).unwrap();
        let url = ServoUrl::parse("https://wusyong.github.io/").unwrap();
        init_servo
            .servo
            .handle_events(vec![EmbedderEvent::NewWebView(url, init_servo.browser_id)]);
        init_servo.servo.setup_logging();
        Verso {
            servo: Some(init_servo.servo),
            webview,
            events: vec![],
            status: Status::None,
        }
    }

    /// Run an iteration of Verso handling cycle. An iteration will perform following actions:
    ///
    /// - Hnadle Winit's event, create Servo's embedder event and push to Verso's event queue.
    /// - Consume Servo's messages and then send all embedder events to Servo.
    /// - And the last step is returning the status of Verso.
    pub fn run(&mut self, event: Event<()>) -> Status {
        self.handle_winit_event(event);
        self.handle_servo_messages();
        log::trace!("Verso sets status to: {:?}", self.status);
        self.status
    }

    fn handle_winit_event(&mut self, event: Event<()>) {
        log::trace!("Verso is creating ebedder event from: {event:?}");
        match event {
            Event::Suspended => {
                self.status = Status::None;
            }
            Event::Resumed | Event::UserEvent(()) => {
                self.events.push(EmbedderEvent::Idle);
            }
            Event::WindowEvent {
                window_id: _,
                event,
            } => self
                .webview
                .handle_winit_window_event(&mut self.servo, &mut self.events, &event),
            e => log::warn!("Verso hasn't supported this event yet: {e:?}"),
        }
    }

    fn handle_servo_messages(&mut self) {
        let Some(servo) = self.servo.as_mut() else {
            return;
        };

        let mut need_present = false;

        servo.get_events().into_iter().for_each(|(w, m)| {
            log::trace!("Verso is handling servo message: {m:?} with browser id: {w:?}");
            match m {
                EmbedderMsg::WebViewOpened(w) => {
                    self.events.push(EmbedderEvent::FocusWebView(w));
                }
                EmbedderMsg::ReadyToPresent(_w) => {
                    need_present = true;
                }
                EmbedderMsg::SetCursor(cursor) => {
                    let winit_cursor = match cursor {
                        Cursor::Default => CursorIcon::Default,
                        Cursor::Pointer => CursorIcon::Pointer,
                        Cursor::ContextMenu => CursorIcon::ContextMenu,
                        Cursor::Help => CursorIcon::Help,
                        Cursor::Progress => CursorIcon::Progress,
                        Cursor::Wait => CursorIcon::Wait,
                        Cursor::Cell => CursorIcon::Cell,
                        Cursor::Crosshair => CursorIcon::Crosshair,
                        Cursor::Text => CursorIcon::Text,
                        Cursor::VerticalText => CursorIcon::VerticalText,
                        Cursor::Alias => CursorIcon::Alias,
                        Cursor::Copy => CursorIcon::Copy,
                        Cursor::Move => CursorIcon::Move,
                        Cursor::NoDrop => CursorIcon::NoDrop,
                        Cursor::NotAllowed => CursorIcon::NotAllowed,
                        Cursor::Grab => CursorIcon::Grab,
                        Cursor::Grabbing => CursorIcon::Grabbing,
                        Cursor::EResize => CursorIcon::EResize,
                        Cursor::NResize => CursorIcon::NResize,
                        Cursor::NeResize => CursorIcon::NeResize,
                        Cursor::NwResize => CursorIcon::NwResize,
                        Cursor::SResize => CursorIcon::SResize,
                        Cursor::SeResize => CursorIcon::SeResize,
                        Cursor::SwResize => CursorIcon::SwResize,
                        Cursor::WResize => CursorIcon::WResize,
                        Cursor::EwResize => CursorIcon::EwResize,
                        Cursor::NsResize => CursorIcon::NsResize,
                        Cursor::NeswResize => CursorIcon::NeswResize,
                        Cursor::NwseResize => CursorIcon::NwseResize,
                        Cursor::ColResize => CursorIcon::ColResize,
                        Cursor::RowResize => CursorIcon::RowResize,
                        Cursor::AllScroll => CursorIcon::AllScroll,
                        Cursor::ZoomIn => CursorIcon::ZoomIn,
                        Cursor::ZoomOut => CursorIcon::ZoomOut,
                        _ => CursorIcon::Default,
                    };
                    self.webview.window.set_cursor_icon(winit_cursor);
                }
                EmbedderMsg::AllowNavigationRequest(pipeline_id, _url) => {
                    if w.is_some() {
                        self.events
                            .push(EmbedderEvent::AllowNavigationResponse(pipeline_id, true));
                    }
                }
                EmbedderMsg::WebViewClosed(_w) => {
                    self.events.push(EmbedderEvent::Quit);
                }
                EmbedderMsg::Shutdown => {
                    self.status = Status::Shutdown;
                }
                e => {
                    log::warn!("Verso hasn't supported handling this message yet: {e:?}")
                }
            }
        });

        log::trace!("Verso is handling embedder events: {:?}", self.events);
        if servo.handle_events(self.events.drain(..)) {
            servo.repaint_synchronously();
            self.webview.paint(servo);
        } else if need_present {
            self.webview.request_redraw();
        }

        if let Status::Shutdown = self.status {
            log::trace!("Verso is shutting down Servo");
            self.servo.take().map(Servo::deinit);
        } else if !self.webview.is_animating() {
            self.status = Status::None;
        }
    }

    /// Helper method to access Servo instance. This can be used to check if Servo is shut down as well.
    pub fn servo(&mut self) -> &mut Option<Servo<WebView>> {
        &mut self.servo
    }

    /// Tell Verso to shutdown Servo safely.
    pub fn shutdown(&mut self) {
        self.events.push(EmbedderEvent::Quit);
    }
}

/// Embedder is required by Servo creation. Servo will use this type to wake up winit's event loop.
#[derive(Debug, Clone)]
struct Embedder(pub EventLoopProxy<()>);

impl EmbedderMethods for Embedder {
    fn create_event_loop_waker(&mut self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }
}

impl EventLoopWaker for Embedder {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        if let Err(e) = self.0.send_event(()) {
            log::error!(
                "Servo embedder failed to send wake up event to Verso: {}",
                e
            );
        }
    }
}
