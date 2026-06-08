/// Open file for reading only.
pub const O_RDONLY: u32 = 0;
/// Open file for writing only.
pub const O_WRONLY: u32 = 1;
/// Open file for reading and writing.
pub const O_RDWR: u32 = 2;
/// Create the file if it does not exist.
pub const O_CREAT: u32 = 0x40;

/// Mask to extract the access mode (read/write) from flags.
const O_ACCMODE: u32 = 3;

/// Returns `true` if the given flags permit reading.
pub fn can_read(flags: u32) -> bool {
    let mode = flags & O_ACCMODE;
    mode == O_RDONLY || mode == O_RDWR
}

/// Returns `true` if the given flags permit writing.
pub fn can_write(flags: u32) -> bool {
    let mode = flags & O_ACCMODE;
    mode == O_WRONLY || mode == O_RDWR
}
