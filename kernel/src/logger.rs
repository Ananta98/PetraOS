use log::{Level, LevelFilter, Metadata, Record};
use ostd::prelude::println;

pub struct KernelLogger;

static LOGGER: KernelLogger = KernelLogger;

pub fn init() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(LevelFilter::Trace);
}

impl log::Log for KernelLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let color = match record.level() {
                Level::Error => "\x1b[31;1m", // Bright Red
                Level::Warn => "\x1b[33;1m",  // Bright Yellow
                Level::Info => "\x1b[32;1m",  // Bright Green
                Level::Debug => "\x1b[34;1m", // Bright Blue
                Level::Trace => "\x1b[35;1m", // Bright Magenta
            };
            let reset = "\x1b[0m";
            println!(
                "{}[{:5}]{} {}: {}",
                color,
                record.level(),
                reset,
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}
