/// Returns true if this process has Accessibility (AXIsProcessTrusted) permission.
/// Without this, CGEventTap creation will fail.
pub fn has_accessibility() -> bool {
    unsafe {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }
        AXIsProcessTrusted()
    }
}

/// Opens System Settings to the Accessibility pane so the user can grant access.
pub fn open_accessibility_settings() {
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Triggers the macOS microphone permission prompt if not yet decided.
/// The result is delivered asynchronously; check `has_microphone()` afterward.
pub fn request_microphone_access() {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, Bool};
    use objc2_foundation::NSString;

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    unsafe {
        let cls = match AnyClass::get(c"AVCaptureDevice") {
            Some(c) => c,
            None => return,
        };
        let media_type = NSString::from_str("soun");
        let block = RcBlock::new(|granted: Bool| {
            tracing::info!("microphone access granted: {}", granted.as_bool());
        });
        let _: () = msg_send![cls, requestAccessForMediaType: &*media_type completionHandler: &*block];
    }
}

/// Returns true if the app has been granted microphone permission.
/// Uses AVCaptureDevice +authorizationStatusForMediaType: to check the real status.
pub fn has_microphone() -> bool {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;
    use objc2_foundation::NSString;

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    unsafe {
        let cls = match AnyClass::get(c"AVCaptureDevice") {
            Some(c) => c,
            None => return false,
        };
        // AVMediaTypeAudio = "soun"
        let media_type = NSString::from_str("soun");
        let status: i64 = msg_send![cls, authorizationStatusForMediaType: &*media_type];
        status == 1 // AVAuthorizationStatusAuthorized = 1
    }
}
