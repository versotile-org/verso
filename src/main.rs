// Prevent console window from appearing on Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use versoview::config::{ArgError, Config};
use versoview::verso::EventLoopProxyMessage;
use versoview::Verso;
use winit::application::ApplicationHandler;
use winit::event_loop::{self, DeviceEvents};
use winit::event_loop::{EventLoop, EventLoopProxy};

struct App {
    /// Config parsed from the command line. `config` XOR `verso` must be `Some`.
    config: Option<Config>,
    /// The verso browser handle (possibly uninitialized).
    verso: Option<Verso>,
    proxy: EventLoopProxy<EventLoopProxyMessage>,
}

impl ApplicationHandler<EventLoopProxyMessage> for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(config) = self.config.take() {
            self.verso = Some(Verso::new(config, event_loop, self.proxy.clone()));
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if let Some(v) = self.verso.as_mut() {
            v.handle_window_event(event_loop, window_id, event);
        }
    }

    fn user_event(
        &mut self,
        event_loop: &event_loop::ActiveEventLoop,
        event: EventLoopProxyMessage,
    ) {
        if let Some(v) = self.verso.as_mut() {
            match event {
                EventLoopProxyMessage::Wake => {
                    v.request_redraw(event_loop);
                }
                EventLoopProxyMessage::IpcMessage(message) => {
                    v.handle_incoming_webview_message(message);
                }
            }
        }
    }
}

fn run() -> versoview::errors::Result<()> {
    let config = Some(versoview::config::Config::new()?);
    init_crypto();

    let event_loop = EventLoop::<EventLoopProxyMessage>::with_user_event().build()?;
    event_loop.listen_device_events(DeviceEvents::Never);
    let proxy = event_loop.create_proxy();
    let mut app = App {
        config,
        verso: None,
        proxy,
    };
    event_loop.run_app(&mut app)?;

    Ok(())
}

fn main() -> ExitCode {
    run().map_or_else(
        |e| match e {
            versoview::Error::ArgError(ArgError::Help(message)) => {
                println!("{message}");
                ExitCode::SUCCESS
            }
            e => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        },
        |()| ExitCode::SUCCESS,
    )
}

fn init_crypto() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Error initializing crypto provider");
}
