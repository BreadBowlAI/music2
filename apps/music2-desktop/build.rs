fn main() {
    // Declare the app's own command so the capability can allow it for the UI's
    // 127.0.0.1 origin (remote origins get no app commands by default).
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&["action"])))
        .expect("tauri build");
}
