use crate::{app::KvikkApp, platform};
use eframe::NativeOptions;
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSURL};
use std::path::PathBuf;
use winit::event_loop::EventLoop;

// Apple Event four-character codes. Finder sends kAEOpenDocuments ('odoc')
// inside kCoreEventClass ('aevt'), with the files in keyDirectObject ('----').
const K_CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const K_AE_OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
const KEY_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

declare_class!(
    struct KvikkOpenDocumentsHandler;

    unsafe impl ClassType for KvikkOpenDocumentsHandler {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "KvikkOpenDocumentsHandler";
    }

    impl DeclaredClass for KvikkOpenDocumentsHandler {}
    unsafe impl NSObjectProtocol for KvikkOpenDocumentsHandler {}

    // `declare_class!` in objc2 0.5.x requires Objective-C method
    // implementations to be declared in an unsafe inherent impl block.
    unsafe impl KvikkOpenDocumentsHandler {
        #[method(handleOpenDocuments:withReplyEvent:)]
        fn handle_open_documents(&self, event: &NSObject, _reply: &NSObject) {
            // NSAppleEventDescriptor's typed AppleEvent APIs are gated behind
            // CoreServices in newer objc2 bindings. Using Objective-C messages here
            // keeps this tiny bridge compatible with the objc2 0.5.x stack used by
            // eframe/winit 0.36 without replacing Winit's NSApplicationDelegate.
            let direct: Option<Retained<NSObject>> = unsafe {
                msg_send_id![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT]
            };

            let Some(direct) = direct else { return };
            let count: isize = unsafe { msg_send![&*direct, numberOfItems] };

            // Apple Event descriptor lists are one-based.
            for index in 1..=count.max(0) {
                let descriptor: Option<Retained<NSObject>> = unsafe {
                    msg_send_id![&*direct, descriptorAtIndex: index]
                };
                let Some(descriptor) = descriptor else { continue };

                let url: Option<Retained<NSURL>> = unsafe {
                    msg_send_id![&*descriptor, fileURLValue]
                };
                let Some(url) = url else { continue };

                // SAFETY: AppKit/Foundation supplied this NSURL as part of the
                // currently handled open-documents Apple Event, and it remains valid
                // for the duration of this callback.
                let ns_path = unsafe { url.path() };
                let Some(ns_path) = ns_path else { continue };

                let path = PathBuf::from(ns_path.to_string());
                if is_pdf(&path) {
                    platform::enqueue_open(path);
                }
            }
        }
    }
);

impl KvikkOpenDocumentsHandler {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send_id![super(mtm.alloc().set_ivars(())), init] }
    }
}

/// Owns the Objective-C Apple Event target for as long as the event loop runs.
/// NSAppleEventManager does not give us a Rust lifetime tying the registered
/// target to the manager, so keeping the Retained object in this scope is the
/// simplest way to make that relationship explicit.
struct OpenDocumentsRegistration {
    _handler: Retained<KvikkOpenDocumentsHandler>,
    _manager: Retained<NSObject>,
}

fn register_open_documents(mtm: MainThreadMarker) -> OpenDocumentsRegistration {
    let handler = KvikkOpenDocumentsHandler::new(mtm);
    let manager_class = AnyClass::get("NSAppleEventManager")
        .expect("Foundation must provide NSAppleEventManager on macOS");
    let manager: Retained<NSObject> = unsafe {
        msg_send_id![manager_class, sharedAppleEventManager]
    };

    // SAFETY: The selector is implemented by KvikkOpenDocumentsHandler with the
    // exact two-object signature required by NSAppleEventManager. The event class
    // and ID are the documented kCoreEventClass/kAEOpenDocuments four-char codes.
    unsafe {
        let _: () = msg_send![
            &*manager,
            setEventHandler: &*handler
            andSelector: sel!(handleOpenDocuments:withReplyEvent:)
            forEventClass: K_CORE_EVENT_CLASS
            andEventID: K_AE_OPEN_DOCUMENTS
        ];
    }

    OpenDocumentsRegistration {
        _handler: handler,
        _manager: manager,
    }
}

pub fn run(startup_path: Option<PathBuf>, options: NativeOptions) -> eframe::Result {
    // Build Winit first. Winit 0.30.13 installs and expects its own
    // NSApplicationDelegate; replacing it causes a startup panic. We therefore
    // leave that delegate untouched and listen for Finder's open-document Apple
    // Event through NSAppleEventManager instead.
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event().build()?;
    let mtm = MainThreadMarker::new().expect("kvikk must start on the macOS main thread");
    let _open_documents = register_open_documents(mtm);

    let mut winit_app = eframe::create_native(
        "kvikk pdf",
        options,
        Box::new(move |cc| Ok(Box::new(KvikkApp::new(cc, startup_path)))),
        &event_loop,
    );

    event_loop.run_app(&mut winit_app)?;
    Ok(())
}

fn is_pdf(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}
