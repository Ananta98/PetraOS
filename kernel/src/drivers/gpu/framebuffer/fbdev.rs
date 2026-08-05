use crate::drivers::char::CharDevice;
use crate::drivers::gpu::framebuffer::color::PixelFormat;
use crate::drivers::gpu::framebuffer::draw::Framebuffer;
use crate::fs::vfs::SeekFrom;
use crate::vm::vma::VmaManager;
use alloc::sync::Arc;
use ostd::mm::VmIo;
use ostd::Error;

pub const FBIOGET_VSCREENINFO: usize = 0x4600;
pub const FBIOPUT_VSCREENINFO: usize = 0x4601;
pub const FBIOGET_FSCREENINFO: usize = 0x4602;

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct FbBitfield {
    pub offset: u32,
    pub length: u32,
    pub msb_right: u32,
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct FbVarScreeninfo {
    pub xres: u32,
    pub yres: u32,
    pub xres_virtual: u32,
    pub yres_virtual: u32,
    pub xoffset: u32,
    pub yoffset: u32,

    pub bits_per_pixel: u32,
    pub grayscale: u32,

    pub red: FbBitfield,
    pub green: FbBitfield,
    pub blue: FbBitfield,
    pub transp: FbBitfield,

    pub nonstd: u32,
    pub activate: u32,

    pub height: u32,
    pub width: u32,

    pub accel_flags: u32,

    pub pixclock: u32,
    pub left_margin: u32,
    pub right_margin: u32,
    pub upper_margin: u32,
    pub lower_margin: u32,
    pub hsync_len: u32,
    pub vsync_len: u32,
    pub sync: u32,
    pub vmode: u32,
    pub rotate: u32,
    pub colorspace: u32,
    pub reserved: [u32; 4],
}

impl FbVarScreeninfo {
    pub fn to_bytes(&self) -> [u8; 160] {
        let mut buf = [0u8; 160];
        buf[0..4].copy_from_slice(&self.xres.to_ne_bytes());
        buf[4..8].copy_from_slice(&self.yres.to_ne_bytes());
        buf[8..12].copy_from_slice(&self.xres_virtual.to_ne_bytes());
        buf[12..16].copy_from_slice(&self.yres_virtual.to_ne_bytes());
        buf[16..20].copy_from_slice(&self.xoffset.to_ne_bytes());
        buf[20..24].copy_from_slice(&self.yoffset.to_ne_bytes());
        buf[24..28].copy_from_slice(&self.bits_per_pixel.to_ne_bytes());
        buf[28..32].copy_from_slice(&self.grayscale.to_ne_bytes());

        // red
        buf[32..36].copy_from_slice(&self.red.offset.to_ne_bytes());
        buf[36..40].copy_from_slice(&self.red.length.to_ne_bytes());
        buf[40..44].copy_from_slice(&self.red.msb_right.to_ne_bytes());

        // green
        buf[44..48].copy_from_slice(&self.green.offset.to_ne_bytes());
        buf[48..52].copy_from_slice(&self.green.length.to_ne_bytes());
        buf[52..56].copy_from_slice(&self.green.msb_right.to_ne_bytes());

        // blue
        buf[56..60].copy_from_slice(&self.blue.offset.to_ne_bytes());
        buf[60..64].copy_from_slice(&self.blue.length.to_ne_bytes());
        buf[64..68].copy_from_slice(&self.blue.msb_right.to_ne_bytes());

        // transp
        buf[68..72].copy_from_slice(&self.transp.offset.to_ne_bytes());
        buf[72..76].copy_from_slice(&self.transp.length.to_ne_bytes());
        buf[76..80].copy_from_slice(&self.transp.msb_right.to_ne_bytes());

        buf[80..84].copy_from_slice(&self.nonstd.to_ne_bytes());
        buf[84..88].copy_from_slice(&self.activate.to_ne_bytes());
        buf[88..92].copy_from_slice(&self.height.to_ne_bytes());
        buf[92..96].copy_from_slice(&self.width.to_ne_bytes());
        buf[96..100].copy_from_slice(&self.accel_flags.to_ne_bytes());
        buf[100..104].copy_from_slice(&self.pixclock.to_ne_bytes());
        buf[104..108].copy_from_slice(&self.left_margin.to_ne_bytes());
        buf[108..112].copy_from_slice(&self.right_margin.to_ne_bytes());
        buf[112..116].copy_from_slice(&self.upper_margin.to_ne_bytes());
        buf[116..120].copy_from_slice(&self.lower_margin.to_ne_bytes());
        buf[120..124].copy_from_slice(&self.hsync_len.to_ne_bytes());
        buf[124..128].copy_from_slice(&self.vsync_len.to_ne_bytes());
        buf[128..132].copy_from_slice(&self.sync.to_ne_bytes());
        buf[132..136].copy_from_slice(&self.vmode.to_ne_bytes());
        buf[136..140].copy_from_slice(&self.rotate.to_ne_bytes());
        buf[140..144].copy_from_slice(&self.colorspace.to_ne_bytes());
        for i in 0..4 {
            let start = 144 + i * 4;
            buf[start..start + 4].copy_from_slice(&self.reserved[i].to_ne_bytes());
        }
        buf
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct FbFixScreeninfo {
    pub id: [u8; 16],
    pub smem_start: usize,
    pub smem_len: u32,
    pub type_: u32,
    pub type_aux: u32,
    pub visual: u32,
    pub xpanstep: u16,
    pub ypanstep: u16,
    pub ywrapstep: u16,
    pub line_length: u32,
    pub mmio_start: usize,
    pub mmio_len: u32,
    pub accel: u32,
    pub capabilities: u16,
    pub reserved: [u16; 2],
}

impl FbFixScreeninfo {
    pub fn to_bytes(&self) -> [u8; 72] {
        let mut buf = [0u8; 72];
        buf[0..16].copy_from_slice(&self.id);
        buf[16..24].copy_from_slice(&self.smem_start.to_ne_bytes());
        buf[24..28].copy_from_slice(&self.smem_len.to_ne_bytes());
        buf[28..32].copy_from_slice(&self.type_.to_ne_bytes());
        buf[32..36].copy_from_slice(&self.type_aux.to_ne_bytes());
        buf[36..40].copy_from_slice(&self.visual.to_ne_bytes());
        buf[40..42].copy_from_slice(&self.xpanstep.to_ne_bytes());
        buf[42..44].copy_from_slice(&self.ypanstep.to_ne_bytes());
        buf[44..46].copy_from_slice(&self.ywrapstep.to_ne_bytes());
        buf[48..52].copy_from_slice(&self.line_length.to_ne_bytes());
        buf[52..60].copy_from_slice(&self.mmio_start.to_ne_bytes());
        buf[60..64].copy_from_slice(&self.mmio_len.to_ne_bytes());
        buf[64..68].copy_from_slice(&self.accel.to_ne_bytes());
        buf[68..70].copy_from_slice(&self.capabilities.to_ne_bytes());
        buf[70..72].copy_from_slice(&self.reserved[0].to_ne_bytes());
        buf
    }
}

/// Character device driver for `/dev/fb0` backed by a `Framebuffer`.
pub struct FbDev {
    fb: Arc<Framebuffer>,
}

impl FbDev {
    pub fn new(fb: Arc<Framebuffer>) -> Self {
        Self { fb }
    }

    fn total_size(&self) -> usize {
        let mode = self.fb.mode();
        (mode.height as usize) * (mode.pitch as usize)
    }
}

impl CharDevice for FbDev {
    fn read(&self, buf: &mut [u8]) -> Result<usize, Error> {
        let mut offset = 0;
        self.read_at(buf, &mut offset)
    }

    fn write(&self, buf: &[u8]) -> Result<usize, Error> {
        let mut offset = 0;
        self.write_at(buf, &mut offset)
    }

    fn read_at(&self, buf: &mut [u8], offset: &mut usize) -> Result<usize, Error> {
        let total = self.total_size();
        if *offset >= total {
            return Ok(0);
        }
        let count = core::cmp::min(buf.len(), total - *offset);
        let pixels = self.fb.pixels.lock();
        buf[..count].copy_from_slice(&pixels[*offset..*offset + count]);
        *offset += count;
        Ok(count)
    }

    fn write_at(&self, buf: &[u8], offset: &mut usize) -> Result<usize, Error> {
        let total = self.total_size();
        if *offset >= total {
            return Ok(0);
        }
        let count = core::cmp::min(buf.len(), total - *offset);
        let mut pixels = self.fb.pixels.lock();
        pixels[*offset..*offset + count].copy_from_slice(&buf[..count]);
        if let Some(ref mmio_lock) = self.fb.mmio {
            let mut mmio = mmio_lock.lock();
            let _ = mmio.write_bytes(*offset, &pixels[*offset..*offset + count]);
        }
        *offset += count;
        Ok(count)
    }

    fn seek(&self, pos: SeekFrom, offset: &mut usize) -> Result<usize, Error> {
        let total = self.total_size();
        let new_offset = match pos {
            SeekFrom::Start(off) => off,
            SeekFrom::Current(off) => {
                let res = (*offset as isize) + off;
                if res < 0 {
                    return Err(Error::InvalidArgs);
                }
                res as usize
            }
            SeekFrom::End(off) => {
                let res = (total as isize) + off;
                if res < 0 {
                    return Err(Error::InvalidArgs);
                }
                res as usize
            }
        };
        *offset = new_offset;
        Ok(*offset)
    }

    fn ioctl(&self, cmd: usize, arg: usize, vm: &VmaManager) -> Result<usize, Error> {
        let mode = self.fb.mode();
        match cmd {
            FBIOGET_VSCREENINFO | FBIOPUT_VSCREENINFO => {
                if arg == 0 {
                    return Err(Error::InvalidArgs);
                }
                let (red_off, blue_off) = match mode.format {
                    PixelFormat::Rgba8888 => (0, 16),
                    PixelFormat::Bgra8888 => (16, 0),
                    _ => (0, 16),
                };
                let var_info = FbVarScreeninfo {
                    xres: mode.width,
                    yres: mode.height,
                    xres_virtual: mode.width,
                    yres_virtual: mode.height,
                    xoffset: 0,
                    yoffset: 0,
                    bits_per_pixel: mode.bpp,
                    grayscale: 0,
                    red: FbBitfield {
                        offset: red_off,
                        length: 8,
                        msb_right: 0,
                    },
                    green: FbBitfield {
                        offset: 8,
                        length: 8,
                        msb_right: 0,
                    },
                    blue: FbBitfield {
                        offset: blue_off,
                        length: 8,
                        msb_right: 0,
                    },
                    transp: FbBitfield {
                        offset: 24,
                        length: 8,
                        msb_right: 0,
                    },
                    nonstd: 0,
                    activate: 0,
                    height: 0,
                    width: 0,
                    accel_flags: 0,
                    pixclock: 0,
                    left_margin: 0,
                    right_margin: 0,
                    upper_margin: 0,
                    lower_margin: 0,
                    hsync_len: 0,
                    vsync_len: 0,
                    sync: 0,
                    vmode: 0,
                    rotate: 0,
                    colorspace: 0,
                    reserved: [0; 4],
                };
                vm.copy_to_user(arg, &var_info.to_bytes())?;
                Ok(0)
            }
            FBIOGET_FSCREENINFO => {
                if arg == 0 {
                    return Err(Error::InvalidArgs);
                }
                let mut id = [0u8; 16];
                let name_bytes = b"petra-fb0";
                id[..name_bytes.len()].copy_from_slice(name_bytes);
                let fix_info = FbFixScreeninfo {
                    id,
                    smem_start: 0,
                    smem_len: (mode.height * mode.pitch) as u32,
                    type_: 0,
                    type_aux: 0,
                    visual: 2,
                    xpanstep: 0,
                    ypanstep: 0,
                    ywrapstep: 0,
                    line_length: mode.pitch,
                    mmio_start: 0,
                    mmio_len: 0,
                    accel: 0,
                    capabilities: 0,
                    reserved: [0; 2],
                };
                vm.copy_to_user(arg, &fix_info.to_bytes())?;
                Ok(0)
            }
            _ => Err(Error::InvalidArgs),
        }
    }
}
