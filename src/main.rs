// Prevent console window from appearing on Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use verso::config::Config;
use verso::{Result, Verso};
use winit::event::{Event, StartCause};
use winit::event_loop::{ControlFlow, DeviceEvents};
use winit::{event_loop::EventLoop, window::WindowBuilder};

/* window decoration */
#[cfg(macos)]
use cocoa::appkit::{NSWindow, NSWindowStyleMask, NSWindowTitleVisibility};
#[cfg(macos)]
use objc::runtime::Object;
#[cfg(macos)]
use raw_window_handle::{AppKitWindowHandle, HasRawWindowHandle, RawWindowHandle};
#[cfg(macos)]
use winit::dpi::LogicalPosition;

fn main() -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.listen_device_events(DeviceEvents::Never);
    let proxy = event_loop.create_proxy();
    let mut verso = None;
    event_loop.run(move |event, evl| {
        if let Event::NewEvents(StartCause::Init) = event {
            let window = WindowBuilder::new()
                // .with_decorations(false)
                .build(&evl)
                .expect("Failed to initialize Winit window");

            #[cfg(macos)]
            unsafe {
                let rwh = window.raw_window_handle();
                if let RawWindowHandle::AppKit(AppKitWindowHandle { ns_window, .. }) = rwh {
                    decorate_window(ns_window as *mut Object, LogicalPosition::new(8.0, 40.0));
                }
            }

            let config = Config::new(resources_dir_path().unwrap());
            verso = Some(Verso::new(window, proxy.clone(), config));
        } else {
            if let Some(v) = &mut verso {
                v.run(event);
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

#[cfg(macos)]
pub unsafe fn decorate_window(window: *mut Object, _position: LogicalPosition<f64>) {
    NSWindow::setTitlebarAppearsTransparent_(window, cocoa::base::YES);
    NSWindow::setTitleVisibility_(window, NSWindowTitleVisibility::NSWindowTitleHidden);
    NSWindow::setStyleMask_(
        window,
        NSWindowStyleMask::NSTitledWindowMask
            | NSWindowStyleMask::NSFullSizeContentViewWindowMask
            | NSWindowStyleMask::NSClosableWindowMask
            | NSWindowStyleMask::NSResizableWindowMask
            | NSWindowStyleMask::NSMiniaturizableWindowMask,
    );
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
