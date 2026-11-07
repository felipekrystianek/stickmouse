//! Automatic updates through Sparkle (sparkle-project.org), the usual updater for Mac apps outside
//! the App Store. Sparkle reads the feed named by `SUFeedURL` in Info.plist once a day, shows the
//! release notes, and installs an update only if it is signed with the key in `SUPublicEDKey`.
//!
//! The framework is loaded at run time from Contents/Frameworks, so the binary does not link it:
//! a `cargo run` outside the app bundle simply has no updater.

use std::cell::OnceCell;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSBundle, NSString};

struct Updater {
    controller: Retained<AnyObject>,
    /// Sparkle keeps only a weak reference to its delegate.
    _delegate: Retained<DriverDelegate>,
}

thread_local! {
    static UPDATER: OnceCell<Option<Updater>> = const { OnceCell::new() };
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and DriverDelegate does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct DriverDelegate;

    impl DriverDelegate {
        /// Stickmouse lives in the menu bar, so Sparkle asks how to remind about scheduled updates.
        #[unsafe(method(supportsGentleScheduledUpdateReminders))]
        fn supports_gentle_reminders(&self) -> bool {
            true
        }

        /// Without a Dock icon the app is never in front, so bring it forward for Sparkle's window.
        #[unsafe(method(standardUserDriverWillHandleShowingUpdate:forUpdate:state:))]
        fn will_show_update(&self, _handled: bool, _update: &AnyObject, _state: &AnyObject) {
            NSApplication::sharedApplication(self.mtm()).activate();
        }
    }
);

fn start(mtm: MainThreadMarker) -> Option<Updater> {
    let path = NSBundle::mainBundle()
        .privateFrameworksPath()?
        .stringByAppendingPathComponent(&NSString::from_str("Sparkle.framework"));
    let framework = NSBundle::bundleWithPath(&path)?;
    // SAFETY: loading Sparkle runs its initializers, which have no requirements on the caller.
    if !unsafe { framework.load() } {
        eprintln!("stickmouse: could not load Sparkle, updates are off");
        return None;
    }
    let class = AnyClass::get(c"SPUStandardUpdaterController")?;
    // SAFETY: NSObject's plain initializer.
    let delegate: Retained<DriverDelegate> = unsafe { msg_send![DriverDelegate::alloc(mtm), init] };
    // SAFETY: the documented initializer; a nil updater delegate is allowed.
    let controller: Retained<AnyObject> = unsafe {
        let allocated: Allocated<AnyObject> = msg_send![class, alloc];
        msg_send![
            allocated,
            initWithStartingUpdater: true,
            updaterDelegate: None::<&AnyObject>,
            userDriverDelegate: Some(&*delegate)
        ]
    };
    Some(Updater { controller, _delegate: delegate })
}

/// Starts Sparkle's daily check. Call once on the main thread at launch.
pub fn start_if_bundled(mtm: MainThreadMarker) {
    UPDATER.with(|cell| {
        cell.get_or_init(|| start(mtm));
    });
}

/// Whether the menu can offer "Check for Updates…".
pub fn available() -> bool {
    UPDATER.with(|cell| cell.get().is_some_and(Option::is_some))
}

/// Checks now and shows the result, including "you're up to date".
pub fn check_now() {
    UPDATER.with(|cell| {
        if let Some(Some(updater)) = cell.get() {
            // SAFETY: an IBAction on SPUStandardUpdaterController that accepts a nil sender.
            let _: () = unsafe { msg_send![&*updater.controller, checkForUpdates: None::<&AnyObject>] };
        }
    });
}
