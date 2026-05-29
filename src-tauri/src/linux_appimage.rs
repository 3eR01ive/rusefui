//! Workarounds for Tauri v2 AppImage + WebKitGTK on Linux.
//!
//! Bundled `libwayland-client` in AppImage often conflicts with the host GPU/EGL stack,
//! causing WebKitWebProcess to abort with `EGL_BAD_PARAMETER` and a blank window.
//! Preloading the system Wayland client library fixes this on Arch/Manjaro/Fedora/etc.
//! See: <https://github.com/tauri-apps/tauri/issues/10749>

/// Must run before Tauri/WebKit initializes (called from `main`).
pub fn prepare_webview_env() {
    if std::env::var_os("APPIMAGE").is_none() {
        return;
    }

    preload_system_wayland_client();
    set_default("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
}

fn preload_system_wayland_client() {
    if std::env::var_os("LD_PRELOAD").is_some() {
        return;
    }

    const CANDIDATES: &[&str] = &[
        "/usr/lib/libwayland-client.so.0",
        "/usr/lib/libwayland-client.so",
        "/usr/lib64/libwayland-client.so.0",
        "/usr/lib64/libwayland-client.so",
        "/lib/x86_64-linux-gnu/libwayland-client.so.0",
        "/lib/x86_64-linux-gnu/libwayland-client.so",
    ];

    for path in CANDIDATES {
        if std::path::Path::new(path).exists() {
            std::env::set_var("LD_PRELOAD", path);
            return;
        }
    }
}

fn set_default(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        std::env::set_var(key, value);
    }
}
