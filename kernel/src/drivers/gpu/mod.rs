pub mod fb;

/// Initialize all GPU/display related drivers.
pub fn init() {
    fb::init();
}
