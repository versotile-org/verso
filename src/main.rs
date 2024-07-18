use std::env::current_dir;

use verso::config::Config;
use verso::{Result, Verso};
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
    let window = WindowBuilder::new()
        .with_decorations(false)
        .build(&event_loop)?;

    #[cfg(macos)]
    unsafe {
        let rwh = window.raw_window_handle();
        if let RawWindowHandle::AppKit(AppKitWindowHandle { ns_window, .. }) = rwh {
            decorate_window(ns_window as *mut Object, LogicalPosition::new(8.0, 40.0));
        }
    }

    let config = Config::new(current_dir().unwrap().join("resources"));
    let mut verso = Verso::new(window, event_loop.create_proxy(), config);
    event_loop.run(move |event, evl| {
        verso.run(event);
        if verso.finished_shutting_down() {
            evl.exit();
        } else if verso.is_animating() {
            evl.set_control_flow(ControlFlow::Poll);
        } else {
            evl.set_control_flow(ControlFlow::Wait);
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
