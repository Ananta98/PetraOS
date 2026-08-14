//! Object-Oriented CPIO Archive Parser (SVR4 newc format).
//!
//! Provides pure `no_std` abstractions to inspect, iterate, and extract CPIO archive
//! payloads without external dependencies or panics.

use alloc::string::String;
use alloc::vec::Vec;

/// CPIO Header size in bytes for SVR4 portable format (newc).
pub const CPIO_HEADER_SIZE: usize = 110;

/// Standard magic number for newc format without CRC.
pub const CPIO_MAGIC_NEWC: &[u8; 6] = b"070701";

/// Standard magic number for newc format with CRC.
pub const CPIO_MAGIC_CRC: &[u8; 6] = b"070702";

/// Trailer filename indicating end of archive.
pub const CPIO_TRAILER_NAME: &str = "TRAILER!!!";

/// Errors encountered when parsing CPIO archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpioError {
    BufferTooSmall,
    InvalidMagic,
    InvalidHexField,
    InvalidUtf8Name,
    CorruptedPadding,
}

/// Represents the type of a filesystem node in a CPIO archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpioFileType {
    RegularFile,
    Directory,
    Symlink,
    BlockDevice,
    CharDevice,
    Fifo,
    Socket,
    Other(u32),
}

/// Parsed SVR4 (newc) CPIO Header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpioHeader {
    pub ino: u32,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u32,
    pub mtime: u32,
    pub filesize: usize,
    pub dev_major: u32,
    pub dev_minor: u32,
    pub rdev_major: u32,
    pub rdev_minor: u32,
    pub namesize: usize,
    pub check: u32,
}

impl CpioHeader {
    /// Parse a 110-byte raw SVR4 newc header.
    pub fn parse(raw: &[u8]) -> Result<Self, CpioError> {
        if raw.len() < CPIO_HEADER_SIZE {
            return Err(CpioError::BufferTooSmall);
        }

        let magic = &raw[0..6];
        if magic != CPIO_MAGIC_NEWC && magic != CPIO_MAGIC_CRC {
            return Err(CpioError::InvalidMagic);
        }

        let ino = Self::parse_hex_u32(&raw[6..14])?;
        let mode = Self::parse_hex_u32(&raw[14..22])?;
        let uid = Self::parse_hex_u32(&raw[22..30])?;
        let gid = Self::parse_hex_u32(&raw[30..38])?;
        let nlink = Self::parse_hex_u32(&raw[38..46])?;
        let mtime = Self::parse_hex_u32(&raw[46..54])?;
        let filesize = Self::parse_hex_u32(&raw[54..62])? as usize;
        let dev_major = Self::parse_hex_u32(&raw[62..70])?;
        let dev_minor = Self::parse_hex_u32(&raw[70..78])?;
        let rdev_major = Self::parse_hex_u32(&raw[78..86])?;
        let rdev_minor = Self::parse_hex_u32(&raw[86..94])?;
        let namesize = Self::parse_hex_u32(&raw[94..102])? as usize;
        let check = Self::parse_hex_u32(&raw[102..110])?;

        Ok(Self {
            ino,
            mode,
            uid,
            gid,
            nlink,
            mtime,
            filesize,
            dev_major,
            dev_minor,
            rdev_major,
            rdev_minor,
            namesize,
            check,
        })
    }

    /// Helper to decode 8-character ASCII hex fields.
    fn parse_hex_u32(slice: &[u8]) -> Result<u32, CpioError> {
        let mut val: u32 = 0;
        for &b in slice {
            let digit = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => (b - b'a' + 10) as u32,
                b'A'..=b'F' => (b - b'A' + 10) as u32,
                _ => return Err(CpioError::InvalidHexField),
            };
            val = (val << 4) | digit;
        }
        Ok(val)
    }

    /// Extract the node file type from permission bitmask.
    pub fn file_type(&self) -> CpioFileType {
        match self.mode & 0o170000 {
            0o100000 => CpioFileType::RegularFile,
            0o040000 => CpioFileType::Directory,
            0o120000 => CpioFileType::Symlink,
            0o060000 => CpioFileType::BlockDevice,
            0o020000 => CpioFileType::CharDevice,
            0o010000 => CpioFileType::Fifo,
            0o140000 => CpioFileType::Socket,
            other => CpioFileType::Other(other),
        }
    }

    pub fn is_directory(&self) -> bool {
        self.file_type() == CpioFileType::Directory
    }

    pub fn is_regular_file(&self) -> bool {
        self.file_type() == CpioFileType::RegularFile
    }

    pub fn is_symlink(&self) -> bool {
        self.file_type() == CpioFileType::Symlink
    }
}

/// Represents a single file or directory entry inside a CPIO archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpioEntry<'a> {
    header: CpioHeader,
    name: &'a str,
    data: &'a [u8],
}

impl<'a> CpioEntry<'a> {
    pub fn new(header: CpioHeader, name: &'a str, data: &'a [u8]) -> Self {
        Self { header, name, data }
    }

    pub fn header(&self) -> &CpioHeader {
        &self.header
    }

    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn is_trailer(&self) -> bool {
        self.name == CPIO_TRAILER_NAME
    }

    pub fn is_directory(&self) -> bool {
        self.header.is_directory()
    }

    pub fn is_regular_file(&self) -> bool {
        self.header.is_regular_file()
    }

    pub fn is_symlink(&self) -> bool {
        self.header.is_symlink()
    }
}

/// Iterator over entries in a CPIO archive buffer.
pub struct CpioIterator<'a> {
    data: &'a [u8],
    offset: usize,
    finished: bool,
}

impl<'a> Iterator for CpioIterator<'a> {
    type Item = Result<CpioEntry<'a>, CpioError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished || self.offset >= self.data.len() {
            return None;
        }

        // 1. Ensure header can be read
        let remaining = &self.data[self.offset..];
        if remaining.len() < CPIO_HEADER_SIZE {
            self.finished = true;
            return None;
        }

        let header = match CpioHeader::parse(remaining) {
            Ok(h) => h,
            Err(e) => {
                self.finished = true;
                return Some(Err(e));
            }
        };

        // 2. Parse filename (padded to 4-byte boundary relative to header start)
        let name_start = self.offset + CPIO_HEADER_SIZE;
        let name_end = name_start + header.namesize;
        if name_end > self.data.len() {
            self.finished = true;
            return Some(Err(CpioError::BufferTooSmall));
        }

        let raw_name = &self.data[name_start..name_end];
        // Strip trailing null byte if present
        let trimmed_name = if raw_name.ends_with(&[0]) {
            &raw_name[..raw_name.len() - 1]
        } else {
            raw_name
        };

        let name = match core::str::from_utf8(trimmed_name) {
            Ok(s) => s,
            Err(_) => {
                self.finished = true;
                return Some(Err(CpioError::InvalidUtf8Name));
            }
        };

        let name_pad = (4 - ((CPIO_HEADER_SIZE + header.namesize) % 4)) % 4;
        let data_start = name_end + name_pad;
        let data_end = data_start + header.filesize;

        if data_end > self.data.len() {
            self.finished = true;
            return Some(Err(CpioError::BufferTooSmall));
        }

        let entry_data = &self.data[data_start..data_end];
        let data_pad = (4 - (header.filesize % 4)) % 4;
        self.offset = data_end + data_pad;

        let entry = CpioEntry::new(header, name, entry_data);
        if entry.is_trailer() {
            self.finished = true;
            return None;
        }

        Some(Ok(entry))
    }
}

/// Object-Oriented wrapper representing a complete CPIO archive in memory.
pub struct CpioArchive<'a> {
    data: &'a [u8],
}

impl<'a> CpioArchive<'a> {
    /// Create a new `CpioArchive` referencing the underlying raw memory buffer.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    /// Return an iterator over entries in the archive.
    pub fn entries(&self) -> CpioIterator<'a> {
        CpioIterator {
            data: self.data,
            offset: 0,
            finished: false,
        }
    }

    /// Find an entry by exact relative or normalized path.
    pub fn find_entry(&self, target_path: &str) -> Option<CpioEntry<'a>> {
        let normalized = target_path.trim_start_matches("./").trim_start_matches('/');
        for entry_res in self.entries() {
            if let Ok(entry) = entry_res {
                let entry_clean = entry.name().trim_start_matches("./").trim_start_matches('/');
                if entry_clean == normalized {
                    return Some(entry);
                }
            }
        }
        None
    }

    /// Collect all file paths in the archive into a vector of strings.
    pub fn list_files(&self) -> Result<Vec<String>, CpioError> {
        let mut list = Vec::new();
        for entry_res in self.entries() {
            let entry = entry_res?;
            list.push(String::from(entry.name()));
        }
        Ok(list)
    }
}
