mod compositor;
mod decorator;
mod panel;
mod scene;
mod wallpaper;

use compositor::Compositor;
use space_hal::{DrmKmsBackend, EvdevInputBackend};

fn main() {
    let renderer = match DrmKmsBackend::open_default() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[space-compositor] DRM/KMS init failed: {}", e);
            std::process::exit(1);
        }
    };
    let input = match EvdevInputBackend::open_all() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("[space-compositor] evdev init failed: {}", e);
            std::process::exit(1);
        }
    };

    let mut comp = match Compositor::new(renderer, input) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[space-compositor] failed to bind IPC socket: {}", e);
            std::process::exit(1);
        }
    };

    if std::env::var("SPACE_NO_AUTOSTART").is_err() {
        comp.spawn_app("space-control");
    }

    comp.run();
}
