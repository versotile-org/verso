use arboard::Clipboard;
use servo::{
    compositing::{
        windowing::{EmbedderEvent, EmbedderMethods},
        CompositeTarget,
    },
    embedder_traits::EventLoopWaker,
    servo_url::ServoUrl,
    Servo,
};
use winit::{event::Event, event_loop::EventLoopProxy, window::Window as WinitWindow};

use crate::{
    config::Config,
    window::{GLWindow, Window},
};

/// Status of Verso instance.
#[derive(Clone, Copy, Debug, Default)]
pub enum Status {
    /// Nothing important to Verso at the moment.
    #[default]
    None,
    /// One of the WebViews is animating.
    Animating,
    /// Verso has shut down.
    Shutdown,
}

/// Main entry point of Verso browser.
pub struct Verso {
    servo: Option<Servo<GLWindow>>,
    window: Window,
    events: Vec<EmbedderEvent>,
    status: Status,
    clipboard: Clipboard,
}

impl Verso {
    /// Create a Verso instance from Winit's window and event loop proxy.
    pub fn new(window: WinitWindow, proxy: EventLoopProxy<()>, config: Config) -> Self {
        config.init();

        let mut window = Window::new(window);
        let callback = Box::new(Embedder(proxy));
        let mut init_servo = Servo::new(
            callback,
            window.gl_window(),
            Some(String::from(
                "Mozilla/5.0 (X11; Linux x86_64; rv:109.0) Gecko/20100101 Firefox/119.0",
            )),
            CompositeTarget::Fbo,
        );
        window.set_webview_id(init_servo.browser_id);

        // TODO should extend resource trait to handle local html files
        let path = std::env::current_dir()
            .unwrap()
            .join("resources/panel.html");
        let url = ServoUrl::from_file_path(path.to_str().unwrap()).unwrap();
        init_servo
            .servo
            .handle_events(vec![EmbedderEvent::NewWebView(url, init_servo.browser_id)]);
        init_servo.servo.setup_logging();
        Verso {
            servo: Some(init_servo.servo),
            window,
            events: vec![],
            status: Status::None,
            clipboard: Clipboard::new()
                .expect("Clipboard isn't supported in this platform or desktop environment."),
        }
    }

    /// Run an iteration of Verso handling cycle. An iteration will perform following actions:
    ///
    /// - Handle Winit's event, create Servo's embedder event and push to Verso's event queue.
    /// - Consume Servo's messages and then send all embedder events to Servo.
    /// - And the last step is returning the status of Verso.
    pub fn run(&mut self, event: Event<()>) -> Status {
        self.handle_winit_event(event);
        self.handle_servo_messages();
        log::trace!("Verso sets status to: {:?}", self.status);
        self.status
    }

    fn handle_winit_event(&mut self, event: Event<()>) {
        log::trace!("Verso is creating embedder event from: {event:?}");
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
                .window
                .handle_winit_window_event(&mut self.servo, &mut self.events, &event),
            e => log::warn!("Verso isn't supporting this event yet: {e:?}"),
        }
    }

    fn handle_servo_messages(&mut self) {
        let Some(servo) = self.servo.as_mut() else {
            return;
        };

        let need_present = self.window.handle_servo_messages(
            servo,
            &mut self.events,
            &mut self.status,
            &mut self.clipboard,
        );

        log::trace!("Verso is handling embedder events: {:?}", self.events);
        if servo.handle_events(self.events.drain(..)) {
            servo.repaint_synchronously();
            self.window.paint(servo);
        } else if need_present {
            self.window.request_redraw();
        }

        if let Status::Shutdown = self.status {
            log::trace!("Verso is shutting down Servo");
            self.servo.take().map(Servo::deinit);
        } else if self.window.is_animating() {
            self.status = Status::Animating;
        } else {
            self.status = Status::None;
        }
    }

    /// Helper method to access Servo instance.
    ///
    /// For instance, this could be used to check if Servo was shut down.
    pub fn servo(&mut self) -> &mut Option<Servo<GLWindow>> {
        &mut self.servo
    }

    /// Tell Verso to shut down Servo safely.
    pub fn shutdown(&mut self) {
        self.events.push(EmbedderEvent::Quit);
    }
}

/// Embedder is required by Servo creation. Servo will use this type to wake up Winit's event loop.
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
