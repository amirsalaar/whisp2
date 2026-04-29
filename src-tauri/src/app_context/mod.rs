use objc2_app_kit::NSWorkspace;

/// Returns the bundle identifier of the frontmost application, if any.
/// Must be called from the main thread.
pub fn frontmost_bundle_id() -> Option<String> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let bundle_id = app.bundleIdentifier()?;
    Some(bundle_id.to_string())
}
