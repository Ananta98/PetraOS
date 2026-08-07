use crate::drivers::serial::{PortIoBackend, SerialPort};
use crate::device::{CharDevice, Device};
use crate::sync::spinlock::Spinlock;
use log::{Log, Metadata, Record, Level};
use core::fmt::Write;

struct Logger {
    serial: Spinlock<Option<SerialPort<PortIoBackend>>>,
}

struct SerialWriter<'a>(&'a mut SerialPort<PortIoBackend>);

impl Write for SerialWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                let _ = self.0.write_byte(b'\r');
            }
            let _ = self.0.write_byte(byte);
        }
        Ok(())
    }
}

static LOGGER: Logger = Logger {
    serial: Spinlock::new(None),
};

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let mut guard = self.serial.lock();
            if let Some(ref mut serial) = *guard {
                let mut writer = SerialWriter(serial);
                let level_str = match record.level() {
                    Level::Error => "\x1B[31m[ERROR]\x1B[0m",
                    Level::Warn  => "\x1B[33m[WARN]\x1B[0m",
                    Level::Info  => "\x1B[32m[INFO]\x1B[0m",
                    Level::Debug => "\x1B[36m[DEBUG]\x1B[0m",
                    Level::Trace => "\x1B[35m[TRACE]\x1B[0m",
                };
                let _ = writeln!(
                    writer,
                    "{} {}: {}",
                    level_str,
                    record.target(),
                    record.args()
                );
            }
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let mut serial = SerialPort::new(PortIoBackend::new(0x3F8)); // COM1 port
    if serial.init().is_ok() {
        {
            let mut guard = LOGGER.serial.lock();
            *guard = Some(serial);
        }
        
        // Register the logger with the log crate
        // SAFETY: set_logger is safe or unsafe depending on version; we handle both using an unsafe block.
        #[allow(unused_unsafe)]
        if let Ok(()) = unsafe { log::set_logger(&LOGGER) } {
            log::set_max_level(log::LevelFilter::Trace);
        }
    }
}