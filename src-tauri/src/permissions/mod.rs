// ── Microphone (iOS + macOS via AVFoundation) ──────────────────────────────

/// Returns true if microphone permission is granted.
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
        status == 3
    }
}

/// Triggers the system microphone permission prompt (NotDetermined),
/// or opens Settings if already denied/restricted.
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
            3 => {} // already authorized
            1 | 2 => open_microphone_settings(),
            _ => {
                let block = RcBlock::new(|granted: Bool| {
                    tracing::info!("microphone access granted: {}", granted.as_bool());
                });
                let _: () = msg_send![cls, requestAccessForMediaType: &*media_type, completionHandler: &*block];
            }
        }
    }
}

/// Opens the platform microphone settings pane.
pub fn open_microphone_settings() {
    #[cfg(target_os = "macos")]
    {
        let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "ios")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyClass;
        use objc2_foundation::NSString;
        unsafe {
            let url_str = NSString::from_str("app-settings:Privacy-Microphone");
            let url_cls = AnyClass::get(c"NSURL").unwrap();
            let url: *mut objc2::runtime::AnyObject =
                msg_send![url_cls, URLWithString: &*url_str];
            let app_cls = AnyClass::get(c"UIApplication").unwrap();
            let app: *mut objc2::runtime::AnyObject = msg_send![app_cls, sharedApplication];
            let _: () = msg_send![app, openURL: url, options: std::ptr::null::<objc2::runtime::AnyObject>(), completionHandler: std::ptr::null::<objc2::runtime::AnyObject>()];
        }
    }
}

// ── macOS-only permissions ──────────────────────────────────────────────────

/// Returns true if Accessibility (AXIsProcessTrusted) permission is granted.
#[cfg(target_os = "macos")]
pub fn has_accessibility() -> bool {
    unsafe {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }
        AXIsProcessTrusted()
    }
}

#[cfg(target_os = "macos")]
pub fn open_accessibility_settings() {
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Returns true if Input Monitoring permission is granted.
#[cfg(target_os = "macos")]
pub fn has_input_monitoring() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightListenEventAccess() -> bool;
    }
    unsafe { CGPreflightListenEventAccess() }
}

#[cfg(target_os = "macos")]
pub fn request_input_monitoring() {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGRequestListenEventAccess() -> bool;
    }
    let granted = unsafe { CGRequestListenEventAccess() };
    tracing::info!("input monitoring request result: {}", granted);
}

#[cfg(target_os = "macos")]
pub fn open_input_monitoring_settings() {
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
    let _ = std::process::Command::new("open").arg(url).spawn();
}
