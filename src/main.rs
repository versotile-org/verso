use winit::{
    dpi::PhysicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use yippee::Yippee;

/* window decoration */
#[cfg(target_os = "macos")]
use cocoa::appkit::{NSView, NSWindow};
#[cfg(target_os = "macos")]
use cocoa::appkit::{NSWindowStyleMask, NSWindowTitleVisibility};
#[cfg(target_os = "macos")]
use objc::{msg_send, runtime::Object, sel, sel_impl};
#[cfg(target_os = "macos")]
use raw_window_handle::{AppKitWindowHandle, HasRawWindowHandle, RawWindowHandle};
#[cfg(target_os = "macos")]
use winit::dpi::LogicalPosition;
#[cfg(target_os = "macos")]
use winit::platform::macos::WindowBuilderExtMacOS;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title("(*ﾟ▽ﾟ)ﾉ Yippee")
        .with_inner_size(PhysicalSize::new(1000, 500))
        .build(&event_loop)
        .unwrap();

    #[cfg(target_os = "macos")]
    unsafe {
        let rwh = window.raw_window_handle();
        if let RawWindowHandle::AppKit(AppKitWindowHandle { ns_window, .. }) = rwh {
            decorate_window(ns_window as *mut Object, LogicalPosition::new(8.0, 40.0));
        }
    }

    #[allow(unused_mut)]
    let mut webview = Yippee::new(window, event_loop.create_proxy());
    event_loop
        .run(move |event, evl| {
            if !evl.exiting() && webview.is_shutdown() {
                if let Some(servo) = webview.servo().take() {
                    servo.deinit();
                }
                evl.exit();
            } else {
                webview.set_control_flow(&event, evl);
                webview.handle_winit_event(event);
                webview.handle_servo_messages();
            }
        })
        .unwrap();
}

#[cfg(target_os = "macos")]
pub unsafe fn decorate_window(window: *mut Object, position: LogicalPosition<f64>) {
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
