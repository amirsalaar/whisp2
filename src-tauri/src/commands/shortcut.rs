/// Presents the iOS "Open in…" share sheet for the bundled
/// `RecordAndTranscribe.shortcut`, letting the user pick Shortcuts.app to
/// install it. iOS-only.
///
/// The actual presentation lives in Swift (WhispIntent.swift) because
/// UIDocumentInteractionController needs a UIView/UIViewController anchor that
/// is awkward to obtain from objc2. UIApplication.openURL(file://) doesn't work
/// here — Shortcuts.app is sandbox-blocked from reading another app's bundle.
#[tauri::command]
pub async fn install_shortcut(
    #[cfg_attr(target_os = "macos", allow(unused_variables))] app: tauri::AppHandle,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return Err("install_shortcut is iOS-only".into());

    #[cfg(target_os = "ios")]
    {
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        app.run_on_main_thread(move || {
            // SAFETY: extern symbol provided by Swift via @_cdecl. Returns a
            // Bool (Swift Bool == 1-byte true/false on iOS ABI).
            extern "C" {
                fn whisp_present_shortcut_installer() -> bool;
            }
            let ok = unsafe { whisp_present_shortcut_installer() };
            let _ = tx.send(if ok {
                Ok(())
            } else {
                Err("Could not present the install sheet. The bundled shortcut file may be missing or no app is registered to open it.".to_string())
            });
        })
        .map_err(|e| format!("run_on_main_thread failed: {e}"))?;
        rx.await
            .map_err(|e| format!("main-thread task dropped: {e}"))?
    }
}
