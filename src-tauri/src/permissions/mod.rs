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

/// Returns true if the app has microphone permission.
pub fn has_microphone() -> bool {
    // cpal will surface the error naturally if mic is denied.
    // Full AVCaptureDevice auth check deferred to v2.
    true
}
