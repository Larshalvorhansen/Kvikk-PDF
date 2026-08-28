#import <AppKit/AppKit.h>
#import <objc/runtime.h>

// Implemented in src/macos.rs. The queue is safe to use before egui exists;
// Kvikk drains it on the first UI frame and wakes egui on later opens.
extern void kvikk_enqueue_open_path_utf8(const char *path);

static void kvikk_enqueue_path(NSString *path) {
    if (path == nil || path.length == 0) {
        return;
    }
    kvikk_enqueue_open_path_utf8(path.fileSystemRepresentation);
}

static BOOL kvikk_application_open_file(
    id self,
    SEL _cmd,
    NSApplication *application,
    NSString *filename
) {
    (void)self;
    (void)_cmd;
    (void)application;
    kvikk_enqueue_path(filename);
    // This return value is important: Finder shows “cannot open this document”
    // when the application delegate reports failure.
    return YES;
}

static void kvikk_application_open_files(
    id self,
    SEL _cmd,
    NSApplication *application,
    NSArray<NSString *> *filenames
) {
    (void)self;
    (void)_cmd;
    for (NSString *filename in filenames) {
        kvikk_enqueue_path(filename);
    }
    [application replyToOpenOrPrint:NSApplicationDelegateReplySuccess];
}

static void kvikk_application_open_urls(
    id self,
    SEL _cmd,
    NSApplication *application,
    NSArray<NSURL *> *urls
) {
    (void)self;
    (void)_cmd;
    (void)application;
    for (NSURL *url in urls) {
        if (url.isFileURL) {
            kvikk_enqueue_path(url.path);
        }
    }
}

static const char *kvikk_delegate_method_types(SEL selector, const char *fallback) {
    Protocol *protocol = @protocol(NSApplicationDelegate);
    struct objc_method_description description =
        protocol_getMethodDescription(protocol, selector, NO, YES);
    return description.types != NULL ? description.types : fallback;
}

static void kvikk_replace_method(Class cls, SEL selector, IMP implementation, const char *fallbackTypes) {
    // Use the protocol's own type encoding. In particular, BOOL has different
    // Objective-C encodings on Apple Silicon and Intel Macs; hard-coding it is
    // exactly the sort of tiny ABI landmine a PDF viewer does not need.
    const char *types = kvikk_delegate_method_types(selector, fallbackTypes);

    // class_replaceMethod() also adds the method when the class does not already
    // implement it. We patch Winit's existing delegate instead of replacing the
    // delegate object, which keeps Winit's lifecycle assumptions intact.
    class_replaceMethod(cls, selector, implementation, types);
}

void kvikk_install_open_handlers(void) {
    NSApplication *application = NSApplication.sharedApplication;
    id delegate = application.delegate;
    if (delegate == nil) {
        return;
    }

    Class delegateClass = object_getClass(delegate);
    if (delegateClass == Nil) {
        return;
    }

    kvikk_replace_method(
        delegateClass,
        @selector(application:openFile:),
        (IMP)kvikk_application_open_file,
#if __arm64__
        "B@:@@"
#else
        "c@:@@"
#endif
    );
    kvikk_replace_method(
        delegateClass,
        @selector(application:openFiles:),
        (IMP)kvikk_application_open_files,
        "v@:@@"
    );
    kvikk_replace_method(
        delegateClass,
        @selector(application:openURLs:),
        (IMP)kvikk_application_open_urls,
        "v@:@@"
    );
}
