//! Finder sends Apple Events, not argv, including when the app is already running.
use objc2::rc::Retained;
use objc2::{MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSObject,
    NSObjectProtocol, NSString,
};
use std::path::PathBuf;
use std::sync::Mutex;

static FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static CONTEXT: Mutex<Option<eframe::egui::Context>> = Mutex::new(None);

define_class!(
    // SAFETY: NSObject has no additional subclassing requirements; no Drop impl.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    pub struct FileOpenHandler;
    unsafe impl NSObjectProtocol for FileOpenHandler {}
    impl FileOpenHandler {
        #[unsafe(method(applicationLaunching:))]
        fn launching(&self, _notification: &NSNotification) {
            register(self);
        }
        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn handle(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let files = paths(event);
            log::debug!("Finder open request: {} files", files.len());
            FILES.lock().unwrap().extend(files);
            if let Some(ctx) = CONTEXT.lock().unwrap().as_ref() { ctx.request_repaint(); }
        }
    }
);

pub fn install() -> Retained<FileOpenHandler> {
    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    // SAFETY: NSObject's init initializes this fieldless subclass. The selector
    // signature above is the NSAppleEventManager event/reply handler signature.
    unsafe {
        // AppKit installs its own Apple Event handlers when NSApplication is
        // initialized. Initialize it first so it cannot replace this handler.
        let _: *mut objc2::runtime::AnyObject =
            msg_send![objc2::class!(NSApplication), sharedApplication];
        let handler: Retained<FileOpenHandler> = msg_send![FileOpenHandler::alloc(mtm), init];
        for name in [
            "NSApplicationWillFinishLaunchingNotification",
            "NSApplicationDidFinishLaunchingNotification",
        ] {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &handler,
                sel!(applicationLaunching:),
                Some(&NSString::from_str(name)),
                None,
            );
        }
        register(&handler);
        handler
    }
}

pub fn attach(ctx: &eframe::egui::Context, handler: &FileOpenHandler) {
    *CONTEXT.lock().unwrap() = Some(ctx.clone());
    // AppKit has now completed launching the application and installing its
    // document dispatch. Reapply the handler before returning to the event loop.
    register(handler);
}

fn register(handler: &FileOpenHandler) {
    // SAFETY: the selector's event/reply signature is declared above.
    unsafe {
        NSAppleEventManager::sharedAppleEventManager()
            .setEventHandler_andSelector_forEventClass_andEventID(
                handler,
                sel!(handleOpenDocuments:withReplyEvent:),
                u32::from_be_bytes(*b"aevt"),
                u32::from_be_bytes(*b"odoc"),
            );
    }
}

pub fn take_files() -> Vec<PathBuf> {
    std::mem::take(&mut *FILES.lock().unwrap())
}

fn paths(event: &NSAppleEventDescriptor) -> Vec<PathBuf> {
    let Some(items) = event.paramDescriptorForKeyword(u32::from_be_bytes(*b"----")) else {
        return Vec::new();
    };
    (1..=items.numberOfItems())
        .filter_map(|index| {
            items
                .descriptorAtIndex(index)
                .and_then(|d| d.fileURLValue())
                .filter(|url| url.isFileURL())
                .and_then(|url| url.path())
                .map(|path| PathBuf::from(path.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::{NSString, NSURL};

    #[test]
    fn finder_events_preserve_spaces_unicode_and_multiple_paths() {
        let directory = tempfile::tempdir().unwrap();
        let expected = [
            directory.path().join("uscita à.mp4"),
            directory.path().join("second.insv"),
        ];
        for (index, path) in expected.iter().enumerate() {
            std::fs::write(path, index.to_string()).unwrap();
        }
        let list = NSAppleEventDescriptor::listDescriptor();
        for (index, path) in expected.iter().enumerate() {
            let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
            list.insertDescriptor_atIndex(
                &NSAppleEventDescriptor::descriptorWithFileURL(&url),
                (index + 1) as isize,
            );
        }
        let event = NSAppleEventDescriptor::appleEventWithEventClass_eventID_targetDescriptor_returnID_transactionID(
            u32::from_be_bytes(*b"aevt"), u32::from_be_bytes(*b"odoc"), None, -1, 0);
        event.setParamDescriptor_forKeyword(&list, u32::from_be_bytes(*b"----"));
        let opened = paths(&event);
        assert_eq!(opened.len(), 2);
        // Foundation uses decomposed Unicode on macOS; check actual file access,
        // rather than requiring the URL and source path to have identical bytes.
        for (index, path) in opened.iter().enumerate() {
            assert_eq!(std::fs::read_to_string(path).unwrap(), index.to_string());
        }
    }
}
