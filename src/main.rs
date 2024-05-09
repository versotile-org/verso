use verso::{Result, Status, Verso};
use winit::window::Fullscreen;
use winit::{event_loop::EventLoop, window::WindowBuilder};

/* window decoration */
#[cfg(target_os = "macos")]
use cocoa::appkit::NSWindow;
#[cfg(target_os = "macos")]
use cocoa::appkit::{NSWindowStyleMask, NSWindowTitleVisibility};
#[cfg(target_os = "macos")]
use objc::runtime::Object;
#[cfg(target_os = "macos")]
use raw_window_handle::{AppKitWindowHandle, HasRawWindowHandle, RawWindowHandle};
#[cfg(target_os = "macos")]
use winit::dpi::LogicalPosition;

fn main() -> Result<()> {
    let event_loop = EventLoop::new()?;
    let window = WindowBuilder::new()
        .with_title("(*ﾟ▽ﾟ)ﾉ Verso")
        .with_fullscreen(Some(Fullscreen::Borderless(None)))
        .build(&event_loop)?;

    #[cfg(target_os = "macos")]
    unsafe {
        let rwh = window.raw_window_handle();
        if let RawWindowHandle::AppKit(AppKitWindowHandle { ns_window, .. }) = rwh {
            decorate_window(ns_window as *mut Object, LogicalPosition::new(8.0, 40.0));
        }
    }

    let mut verso = Verso::new(window, event_loop.create_proxy());
    event_loop.run(move |event, evl| match verso.run(event, evl) {
        Status::Shutdown => evl.exit(),
        _ => (),
    })?;

    Ok(())
}

#[cfg(target_os = "macos")]
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
