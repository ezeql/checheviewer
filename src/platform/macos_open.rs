//! Opening files from Finder.
//!
//! When an image is double-clicked, or "Open With" is used, macOS does **not**
//! pass the path through `argv`: it sends a `kAEOpenDocuments` Apple Event. An
//! app that only looks at its arguments starts empty, as if it had been
//! opened from the Dock.
//!
//! Here a handler for that event is installed before the window starts. The
//! paths that arrive are stored in a queue that the app drains on the next
//! frame.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock};

use block2::RcBlock;
use objc2_core_foundation::{CFStringBuiltInEncodings, CFURL};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSString};

/// Paths received from the system and not yet consumed.
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// To wake the UI from its nap when a file arrives: without this, egui may
/// be waiting for a mouse event and not redraw.
static REPAINT: OnceLock<egui::Context> = OnceLock::new();

// Four-letter codes from the Apple Events API.
const K_CORE_EVENT_CLASS: u32 = four_cc(b"aevt");
const K_AE_OPEN_DOCUMENTS: u32 = four_cc(b"odoc");
const KEY_DIRECT_OBJECT: u32 = four_cc(b"----");
const TYPE_AE_LIST: u32 = four_cc(b"list");
const TYPE_FILE_URL: u32 = four_cc(b"furl");
const NO_ERR: i16 = 0;

const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

#[repr(C)]
struct AEDesc {
    descriptor_type: u32,
    data_handle: *mut c_void,
}

impl AEDesc {
    const fn null() -> Self {
        Self { descriptor_type: 0, data_handle: std::ptr::null_mut() }
    }
}

type AEEventHandler =
    extern "C-unwind" fn(event: *const AEDesc, reply: *mut AEDesc, refcon: isize) -> i16;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C-unwind" {
    fn AEInstallEventHandler(
        event_class: u32,
        event_id: u32,
        handler: AEEventHandler,
        refcon: isize,
        is_sys_handler: u8,
    ) -> i16;

    fn AEGetParamDesc(event: *const AEDesc, key: u32, desired: u32, result: *mut AEDesc) -> i16;

    fn AECountItems(list: *const AEDesc, count: *mut isize) -> i16;

    #[allow(clippy::too_many_arguments)]
    fn AEGetNthPtr(
        list: *const AEDesc,
        index: isize,
        desired_type: u32,
        key: *mut u32,
        type_code: *mut u32,
        data: *mut c_void,
        maximum_size: isize,
        actual_size: *mut isize,
    ) -> i16;

    fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
}

/// Installs the handler. It must be called before the window starts, so the
/// event the system uses to open the app isn't lost.
pub fn install(ctx_for_repaint: Option<egui::Context>) {
    if let Some(ctx) = ctx_for_repaint {
        let _ = REPAINT.set(ctx);
    }

    // SAFETY: the signature matches what Carbon expects and the handler is a
    // static function that lives for the whole program run.
    let status =
        unsafe { AEInstallEventHandler(K_CORE_EVENT_CLASS, K_AE_OPEN_DOCUMENTS, on_open, 0, 0) };

    if status != NO_ERR {
        // Not fatal: the app keeps working, but opening it from Finder won't
        // bring the file.
        eprintln!("checheviewer: could not listen for Finder open requests (error {status})");
    }
}

/// Reinstalls the handler as soon as AppKit finishes registering its own.
///
/// This is the delicate point of the whole module: `NSApplication` registers
/// its own Apple Event handlers while starting up and overwrites ours, so
/// installing it only from `main` isn't enough. The `willFinishLaunching`
/// notification arrives after that registration and before the event Finder
/// used to open the app is processed, which is exactly the window we need to
/// avoid losing the first file.
pub fn install_on_launch() {
    let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
        install(None);
    });

    // SAFETY: the block captures nothing borrowed and the observer is kept
    // alive for the lifetime of the program.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(&name),
            None,
            None,
            &block,
        )
    };

    // The notification center retains neither the observer nor the block:
    // they have to be kept alive by hand.
    std::mem::forget(observer);
    std::mem::forget(block);
}

/// Registers the egui context once it exists, so the UI can be woken up when
/// a file arrives.
pub fn set_repaint_context(ctx: egui::Context) {
    let _ = REPAINT.set(ctx);
}

/// Takes the last received path, if any.
///
/// If the user selected several images, only one is kept: the app shows the
/// folder that contains it, so the others will be a couple of arrow presses
/// away anyway.
pub fn take_pending() -> Option<PathBuf> {
    PENDING.lock().ok()?.pop()
}

extern "C-unwind" fn on_open(event: *const AEDesc, _reply: *mut AEDesc, _refcon: isize) -> i16 {
    // SAFETY: the system guarantees a valid event for the duration of the call.
    let paths = unsafe { paths_from_event(event) };

    if !paths.is_empty() {
        if let Ok(mut pending) = PENDING.lock() {
            pending.extend(paths);
        }
        if let Some(ctx) = REPAINT.get() {
            ctx.request_repaint();
        }
    }
    NO_ERR
}

/// Extracts the paths from the event's document list.
///
/// # Safety
///
/// `event` must be a valid Apple Event.
unsafe fn paths_from_event(event: *const AEDesc) -> Vec<PathBuf> {
    let mut list = AEDesc::null();
    if unsafe { AEGetParamDesc(event, KEY_DIRECT_OBJECT, TYPE_AE_LIST, &mut list) } != NO_ERR {
        return Vec::new();
    }

    let mut count: isize = 0;
    let mut paths = Vec::new();
    if unsafe { AECountItems(&list, &mut count) } == NO_ERR {
        for index in 1..=count {
            if let Some(path) = unsafe { nth_path(&list, index) } {
                paths.push(path);
            }
        }
    }

    unsafe { AEDisposeDesc(&mut list) };
    paths
}

/// # Safety
///
/// `list` must be a valid Apple Event list and `index` must be in range.
unsafe fn nth_path(list: *const AEDesc, index: isize) -> Option<PathBuf> {
    // File URLs are short; PATH_MAX is more than enough even with percent
    // escaping.
    let mut buffer = [0u8; 4096];
    let mut actual: isize = 0;

    let status = unsafe {
        AEGetNthPtr(
            list,
            index,
            TYPE_FILE_URL,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            buffer.as_mut_ptr() as *mut c_void,
            buffer.len() as isize,
            &mut actual,
        )
    };
    if status != NO_ERR || actual <= 0 {
        return None;
    }

    // A percent-escaped `file://` URL arrives; CoreFoundation converts it to a
    // filesystem path without us having to unescape it by hand.
    let length = actual.min(buffer.len() as isize);
    let url = unsafe {
        CFURL::with_bytes(
            None,
            buffer.as_ptr(),
            length,
            CFStringBuiltInEncodings::EncodingUTF8.0,
            None,
        )
    }?;

    let mut path = [0u8; 4096];
    if !unsafe { url.file_system_representation(true, path.as_mut_ptr(), path.len() as isize) } {
        return None;
    }

    let end = path.iter().position(|&b| b == 0).unwrap_or(path.len());
    if end == 0 {
        return None;
    }

    use std::os::unix::ffi::OsStrExt;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&path[..end]).to_os_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_letter_codes_match_the_system_ones() {
        assert_eq!(K_CORE_EVENT_CLASS, 0x6165_7674); // 'aevt'
        assert_eq!(K_AE_OPEN_DOCUMENTS, 0x6F64_6F63); // 'odoc'
        assert_eq!(KEY_DIRECT_OBJECT, 0x2D2D_2D2D); // '----'
        assert_eq!(TYPE_AE_LIST, 0x6C69_7374); // 'list'
        assert_eq!(TYPE_FILE_URL, 0x6675_726C); // 'furl'
    }

    #[test]
    fn installing_the_handler_does_not_fail() {
        install(None);
        // Neither does installing it twice: it replaces the previous one.
        install(None);
    }

    #[test]
    fn no_pending_files_returns_nothing() {
        while take_pending().is_some() {}
        assert!(take_pending().is_none());
    }
}
