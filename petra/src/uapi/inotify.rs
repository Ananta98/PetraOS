// SPDX-License-Identifier: GPL-2.0

//! UAPI definitions for the Linux `inotify` file monitoring subsystem.
//!
//! Exposes `struct inotify_event`, watch event mask constants, and creation flags.

/// File descriptor creation flags for `inotify_init1(2)`.
pub const IN_CLOEXEC: i32 = 0o2000000;
pub const IN_NONBLOCK: i32 = 0o4000;
pub const IN_INIT_FLAGS_MASK: i32 = IN_CLOEXEC | IN_NONBLOCK;

// --- inotify event mask bits ---

/// File was accessed (read).
pub const IN_ACCESS: u32 = 0x0000_0001;
/// File was modified.
pub const IN_MODIFY: u32 = 0x0000_0002;
/// Metadata changed (e.g., permissions, timestamps, extended attributes).
pub const IN_ATTRIB: u32 = 0x0000_0004;
/// Writable file was closed.
pub const IN_CLOSE_WRITE: u32 = 0x0000_0008;
/// Unwritable file was closed.
pub const IN_CLOSE_NOWRITE: u32 = 0x0000_0010;
/// Close (both writable and unwritable).
pub const IN_CLOSE: u32 = IN_CLOSE_WRITE | IN_CLOSE_NOWRITE;
/// File was opened.
pub const IN_OPEN: u32 = 0x0000_0020;
/// File was moved from X.
pub const IN_MOVED_FROM: u32 = 0x0000_0040;
/// File was moved to Y.
pub const IN_MOVED_TO: u32 = 0x0000_0080;
/// Moves (both from and to).
pub const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;
/// Subfile was created.
pub const IN_CREATE: u32 = 0x0000_0100;
/// Subfile was deleted.
pub const IN_DELETE: u32 = 0x0000_0200;
/// Watched file/directory itself was deleted.
pub const IN_DELETE_SELF: u32 = 0x0000_0400;
/// Watched file/directory itself was moved.
pub const IN_MOVE_SELF: u32 = 0x0000_0800;

/// Backing filesystem unmounted.
pub const IN_UNMOUNT: u32 = 0x0000_2000;
/// Event queue overflowed.
pub const IN_Q_OVERFLOW: u32 = 0x0000_4000;
/// Watch was removed explicitly (rm_watch) or automatically (unmount/delete).
pub const IN_IGNORED: u32 = 0x0000_8000;

/// Only watch the path if it is a directory.
pub const IN_ONLYDIR: u32 = 0x0100_0000;
/// Do not follow a symlink.
pub const IN_DONT_FOLLOW: u32 = 0x0200_0000;
/// Exclude events on unlinked children.
pub const IN_EXCL_UNLINK: u32 = 0x0400_0000;
/// Only create watch if pathname does not already exist.
pub const IN_MASK_CREATE: u32 = 0x1000_0000;
/// Add (OR) events to watch mask for this pathname if it already exists.
pub const IN_MASK_ADD: u32 = 0x2000_0000;
/// Event-subject is a directory.
pub const IN_ISDIR: u32 = 0x4000_0000;
/// Only send event once for this watch.
pub const IN_ONESHOT: u32 = 0x8000_0000;

/// All event types combined.
pub const IN_ALL_EVENTS: u32 = IN_ACCESS
    | IN_MODIFY
    | IN_ATTRIB
    | IN_CLOSE_WRITE
    | IN_CLOSE_NOWRITE
    | IN_OPEN
    | IN_MOVED_FROM
    | IN_MOVED_TO
    | IN_DELETE
    | IN_CREATE
    | IN_DELETE_SELF
    | IN_MOVE_SELF;

/// Header structure for `struct inotify_event` as delivered through `read(2)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct InotifyEventHeader {
    /// Watch descriptor.
    pub wd: i32,
    /// Mask describing event.
    pub mask: u32,
    /// Identifies related events (e.g. rename pairs).
    pub cookie: u32,
    /// Size of `name` field including null bytes and padding.
    pub len: u32,
}
