// Prevent console window from appearing on Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use verso::config::Config;
use verso::{Result, Verso};
use winit::event::{Event, StartCause};
use winit::event_loop::EventLoop;
use winit::event_loop::{ControlFlow, DeviceEvents};

fn main() -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.listen_device_events(DeviceEvents::Never);
    let proxy = event_loop.create_proxy();
    let mut verso = None;
    event_loop.run(move |event, evl| {
        if let Event::NewEvents(StartCause::Init) = event {
            let config = Config::new(resources_dir_path().unwrap());
            verso = Some(Verso::new(evl, proxy.clone(), config));
        } else {
            if let Some(v) = &mut verso {
                v.run(event, evl);
                if v.finished_shutting_down() {
                    evl.exit();
                } else if v.is_animating() {
                    evl.set_control_flow(ControlFlow::Poll);
                } else {
                    evl.set_control_flow(ControlFlow::Wait);
                }
            }
        }
    })?;

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
