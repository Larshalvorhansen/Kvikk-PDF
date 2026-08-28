use crate::{app::KvikkApp, platform};
use eframe::NativeOptions;
use std::{
    ffi::{c_char, CStr},
    path::PathBuf,
};
use winit::event_loop::EventLoop;

extern "C" {
    fn kvikk_install_open_handlers();
}

/// Called by the tiny Objective-C bridge attached to Winit's existing
/// NSApplicationDelegate. Using a C ABI here keeps the macOS document-open
/// plumbing independent from objc2's versioned Rust bindings.
#[no_mangle]
pub extern "C" fn kvikk_enqueue_open_path_utf8(path: *const c_char) {
    if path.is_null() {
        return;
    }

    // SAFETY: macos_bridge.m passes NSString.fileSystemRepresentation, which is
    // a NUL-terminated pointer valid for the duration of this call.
    let path = unsafe { CStr::from_ptr(path) };
    let path = PathBuf::from(path.to_string_lossy().into_owned());
    if is_pdf(&path) {
        platform::enqueue_open(path);
    }
}

pub fn run(startup_path: Option<PathBuf>, options: NativeOptions) -> eframe::Result {
    // Let Winit create its normal NSApplication and delegate first. We then add
    // only the three document-open methods to that delegate's Objective-C class.
    // This avoids the Winit 0.30 delegate-replacement panic while allowing Finder
    // to receive an explicit success result for PDF opens.
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event().build()?;
    unsafe { kvikk_install_open_handlers() };

    let mut winit_app = eframe::create_native(
        "kvikk pdf",
        options,
        Box::new(move |cc| Ok(Box::new(KvikkApp::new(cc, startup_path)))),
        &event_loop,
    );

    // `create_native()` should preserve Winit's delegate, but installing again
    // here is cheap and protects us if native initialization replaced its class.
    unsafe { kvikk_install_open_handlers() };

    event_loop.run_app(&mut winit_app)?;
    Ok(())
}

fn is_pdf(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}
