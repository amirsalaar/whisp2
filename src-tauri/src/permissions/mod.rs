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
/// If already denied/restricted, opens System Settings → Privacy → Microphone instead.
///
/// AVAuthorizationStatus values:
///   0 = NotDetermined, 1 = Restricted, 2 = Denied, 3 = Authorized
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

        let status: i64 = msg_send![cls, authorizationStatusForMediaType: &*media_type];

        match status {
            3 => {
                // Already authorized — nothing to do.
            }
            1 | 2 => {
                // Restricted or Denied — system won't show a prompt; open Settings.
                open_microphone_settings();
            }
            _ => {
                // NotDetermined (0) — trigger the system prompt.
                let block = RcBlock::new(|granted: Bool| {
                    tracing::info!("microphone access granted: {}", granted.as_bool());
                });
                let _: () = msg_send![cls, requestAccessForMediaType: &*media_type completionHandler: &*block];
            }
        }
    }
}

/// Opens System Settings to the Microphone privacy pane.
pub fn open_microphone_settings() {
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Returns true if Input Monitoring permission is granted via CGPreflightListenEventAccess().
/// Returns false for both NotDetermined and Denied — cannot distinguish between them.
pub fn has_input_monitoring() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightListenEventAccess() -> bool;
    }
    unsafe { CGPreflightListenEventAccess() }
}

/// Triggers the macOS Input Monitoring permission prompt if not yet decided.
/// Silently does nothing if already denied — open Settings instead.
pub fn request_input_monitoring() {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGRequestListenEventAccess() -> bool;
    }
    let granted = unsafe { CGRequestListenEventAccess() };
    tracing::info!("input monitoring request result: {}", granted);
}

/// Opens System Settings to the Input Monitoring privacy pane.
pub fn open_input_monitoring_settings() {
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Returns true if the app has been granted microphone permission.
/// Uses AVCaptureDevice +authorizationStatusForMediaType: to check the real status.
///
/// AVAuthorizationStatus: 0=NotDetermined, 1=Restricted, 2=Denied, 3=Authorized
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
        let media_type = NSString::from_str("soun");
        let status: i64 = msg_send![cls, authorizationStatusForMediaType: &*media_type];
        status == 3 // AVAuthorizationStatusAuthorized = 3
    }
}
