// Prevent console window from appearing on Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crossbeam_channel::{Receiver, Sender};
use versoview::config::{Config, EmbedderEvent};
use versoview::verso::EvlProxyMessage;
use versoview::{Result, Verso};
use winit::application::ApplicationHandler;
use winit::event_loop::{self, ControlFlow, DeviceEvents};
use winit::event_loop::{EventLoop, EventLoopProxy};

struct App {
    verso: Option<Verso>,
    proxy: EventLoopProxy<EvlProxyMessage>,
    evl_sender: Sender<EmbedderEvent>,
    evl_receiver: Receiver<EmbedderEvent>,
}

impl ApplicationHandler<EvlProxyMessage> for App {
    fn resumed(&mut self, _: &winit::event_loop::ActiveEventLoop) {
        let config = Config::new(resources_dir_path().unwrap());
        self.verso = Some(Verso::new(
            self.proxy.clone(),
            config,
            self.evl_receiver.clone(),
        ));
    }

    fn window_event(
        &mut self,
        _: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if let Err(_) = self
            .evl_sender
            .send(EmbedderEvent::WindowEvent(window_id, event))
        {
            log::error!("Failed to send EmbedderEvent::WindowEvent");
        }
    }

    fn user_event(&mut self, event_loop: &event_loop::ActiveEventLoop, event: EvlProxyMessage) {
        match event {
            EvlProxyMessage::Wake => {
                println!("???");
            }
            EvlProxyMessage::IpcMessage(message) => {
                todo!();
                // v.handle_incoming_webview_message(message);
            }
            EvlProxyMessage::NewWindow => {
                Verso::create_window(event_loop, &self.evl_sender);
            }
            EvlProxyMessage::NewWindowWithConfig => {
                Verso::create_window_with_config(event_loop, &self.evl_sender);
            }
            EvlProxyMessage::Exit => {
                if let Some(verso) = self.verso.take() {
                    verso.exit();
                }
                event_loop.exit()
            }
            EvlProxyMessage::Poll => event_loop.set_control_flow(ControlFlow::Poll),
            EvlProxyMessage::Wait => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

fn main() -> Result<()> {
    let event_loop = EventLoop::<EvlProxyMessage>::with_user_event().build()?;
    event_loop.listen_device_events(DeviceEvents::Never);
    let proxy = event_loop.create_proxy();
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut app = App {
        verso: None,
        proxy,
        evl_sender: tx,
        evl_receiver: rx,
    };
    event_loop.run_app(&mut app)?;

    Ok(())
}

fn resources_dir_path() -> Option<std::path::PathBuf> {
    #[cfg(feature = "packager")]
    let root_dir = {
        use cargo_packager_resource_resolver::{current_format, resources_dir};
        current_format().and_then(|format| resources_dir(format))
    };
    #[cfg(feature = "flatpak")]
    let root_dir = {
        use std::str::FromStr;
        std::path::PathBuf::from_str("/app")
    };
    #[cfg(not(any(feature = "packager", feature = "flatpak")))]
    let root_dir = std::env::current_dir();

    root_dir.ok().map(|dir| dir.join("resources"))
}
