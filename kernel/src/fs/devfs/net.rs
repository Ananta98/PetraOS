//! Network Interface Device Nodes (/dev/ethN)
//!
//! Exposes probed network interfaces (e.g. the Intel e1000 registered as
//! `eth0`) as character device nodes under `/dev`. Each node supports:
//!
//! * Raw Layer-2 frame I/O — `read(2)` dequeues a single received Ethernet
//!   frame, `write(2)` transmits the buffer as one frame.
//! * `poll(2)` readiness reporting driven by non-destructive RX/TX ring peeks.
//! * Standard `SIOCGIF*` interface ioctls (name, index, hardware address,
//!   flags, MTU, IPv4 address/netmask/broadcast, and `SIOCGIFCONF` enumeration)
//!   so userspace can discover and query interfaces.
//!
//! NOTE: Frames consumed through a raw node bypass the smoltcp network stack.

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::device::{DEVICE_MANAGER, DeviceType};
use crate::fs::vfs::mount::MOUNT_TABLE;
use crate::fs::vfs::types::{FileOps, Inode, InodeOps, InodeType, O_NONBLOCK, Stat, VfsError};
use crate::mm::UserPtr;
use crate::net::NET_STACK;

/// Maximum network interface name length (including NUL), per Linux ABI.
pub const IFNAMSIZ: usize = 16;

/// Ethernet hardware type reported in `SIOCGIFHWADDR` results.
pub const ARPHRD_ETHER: u16 = 1;

/// Standard Ethernet maximum transmission unit.
pub const ETH_MTU: u16 = 1500;

// ===== Interface flag bits (Linux `iff_flags`) =====

pub const IFF_UP: u16 = 0x1;
pub const IFF_BROADCAST: u16 = 0x2;
pub const IFF_LOOPBACK: u16 = 0x8;
pub const IFF_RUNNING: u16 = 0x40;
pub const IFF_NOARP: u16 = 0x80;
pub const IFF_MULTICAST: u16 = 0x1000;

// ===== Interface ioctl command numbers (Linux x86-64 values) =====

pub const SIOCGIFNAME: u64 = 0x8910;
pub const SIOCGIFCONF: u64 = 0x8912;
pub const SIOCGIFFLAGS: u64 = 0x8913;
pub const SIOCGIFADDR: u64 = 0x8915;
pub const SIOCSIFADDR: u64 = 0x8916;
pub const SIOCGIFDSTADDR: u64 = 0x8917;
pub const SIOCGIFBRDADDR: u64 = 0x8919;
pub const SIOCGIFNETMASK: u64 = 0x891B;
pub const SIOCGIFMTU: u64 = 0x8921;
pub const SIOCSIFMTU: u64 = 0x8922;
pub const SIOCGIFHWADDR: u64 = 0x8927;
pub const SIOCGIFINDEX: u64 = 0x8933;

// ===== User-facing ABI structures =====

/// Generic socket address layout of the `struct sockaddr` embedded in an
/// `ifreq` union (`sa_family` + 14 bytes of payload).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IfSockAddr {
    pub family: u16,
    pub sa_data: [u8; 14],
}

impl IfSockAddr {
    /// Build an `AF_INET` sockaddr carrying an IPv4 address (port zero).
    fn ipv4(octets: [u8; 4]) -> Self {
        let mut addr = Self::default();
        addr.family = crate::net::types::AF_INET;
        // struct sockaddr_in embeds sin_port at sa_data[0..2] and sin_addr at
        // sa_data[2..6] when carried through a generic sockaddr.
        addr.sa_data[2..6].copy_from_slice(&octets);
        addr
    }

    /// Build an hardware-address sockaddr carrying a MAC address.
    fn ether(mac: [u8; 6]) -> Self {
        let mut addr = Self::default();
        addr.family = ARPHRD_ETHER;
        addr.sa_data[..6].copy_from_slice(&mac);
        addr
    }
}

/// The `ifr_ifru` union of `struct ifreq`. Only plain-data variants are used,
/// mirroring the fields PetraOS userspace may consume.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfReqData {
    pub addr: IfSockAddr,
    pub hwaddr: IfSockAddr,
    pub flags: u16,
    pub ivalue: i32,
}

/// `struct ifreq` as defined by the Linux ABI (name + 24-byte union).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfReq {
    pub name: [u8; IFNAMSIZ],
    pub data: IfReqData,
}

impl IfReq {
    /// Create an `ifreq` with the given interface name (zero-padded).
    pub fn new(name: &str) -> Self {
        Self {
            name: name_to_bytes(name),
            data: IfReqData {
                addr: IfSockAddr::default(),
            },
        }
    }

    fn set_addr(&mut self, addr: IfSockAddr) {
        self.data.addr = addr;
    }

    fn set_hwaddr(&mut self, hwaddr: IfSockAddr) {
        self.data.hwaddr = hwaddr;
    }

    fn set_flags(&mut self, flags: u16) {
        self.data.flags = flags;
    }

    fn set_ivalue(&mut self, value: i32) {
        self.data.ivalue = value;
    }
}

/// `struct ifconf` for `SIOCGIFCONF` buffer exchange.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IfConf {
    /// Input: buffer capacity in bytes. Output: bytes required/copied.
    pub ifc_len: i32,
    __pad: i32,
    /// User pointer to an array of `IfReq`.
    pub ifc_buf: u64,
}

// ===== Interface enumeration helpers =====

/// Collect the devfs-visible names of all registered network devices, in
/// DEVICE_MANAGER registration order. Index N here is ifindex N+1.
fn interface_names() -> Vec<&'static str> {
    let dm = DEVICE_MANAGER.read();
    dm.get_by_type(DeviceType::Network)
        .iter()
        .filter_map(|device| device.lock().dev_name())
        .collect()
}

/// Resolve an interface index from a zero-padded `ifreq` name. An empty name
/// selects the first interface, matching common userspace expectations.
fn resolve_index(name: &[u8; IFNAMSIZ]) -> Option<usize> {
    let names = interface_names();
    if names.is_empty() {
        return None;
    }

    let trimmed = trim_name(name);
    if trimmed.is_empty() {
        return Some(0);
    }

    names.iter().position(|iface| iface.as_bytes() == trimmed)
}

/// Convert an interface name to its fixed-width zero-padded representation.
fn name_to_bytes(name: &str) -> [u8; IFNAMSIZ] {
    let mut bytes = [0u8; IFNAMSIZ];
    let copy_len = core::cmp::min(name.len(), IFNAMSIZ - 1);
    bytes[..copy_len].copy_from_slice(&name.as_bytes()[..copy_len]);
    bytes
}

/// Trim trailing NUL padding from a fixed-width interface name.
fn trim_name(name: &[u8; IFNAMSIZ]) -> &[u8] {
    match name.iter().position(|&b| b == 0) {
        Some(end) => &name[..end],
        None => &name[..],
    }
}

/// Extract the primary IPv4 address and prefix length from the network stack.
fn ipv4_cidr() -> Option<([u8; 4], u8)> {
    let stack_guard = NET_STACK.lock();
    let stack = stack_guard.as_ref()?;
    stack.iface.ip_addrs().iter().find_map(|cidr| {
        if let smoltcp::wire::IpCidr::Ipv4(cidr) = cidr {
            Some((cidr.address().octets(), cidr.prefix_len()))
        } else {
            None
        }
    })
}

/// Compute the netmask octets for a prefix length.
fn prefix_to_netmask(prefix: u8) -> [u8; 4] {
    let mut mask = [0u8; 4];
    for (index, byte) in mask.iter_mut().enumerate() {
        let base = index as u8 * 8;
        if prefix >= base + 8 {
            *byte = 0xFF;
        } else if prefix > base {
            *byte = 0xFF << (8 - (prefix - base));
        } else {
            break;
        }
    }
    mask
}

// ===== Device node =====

/// Inode for a `/dev/ethN` network interface device.
pub struct NetDeviceInode {
    pub iface_name: &'static str,
}

impl InodeOps for NetDeviceInode {
    fn open(&self) -> Result<Arc<dyn FileOps>, VfsError> {
        // Existence check at open time; raw I/O state lives behind the driver
        // globals (E1000_DEVICE / NET_STACK), not behind this proxy reference.
        let dm = DEVICE_MANAGER.read();
        if dm.get_by_name(self.iface_name).is_none() {
            return Err(VfsError::NotFound);
        }

        Ok(Arc::new(NetDeviceFileOps))
    }

    fn stat(&self) -> Result<Stat, VfsError> {
        Ok(Stat {
            mode: 0o020660, // S_IFCHR | 0660
            nlink: 1,
            ..Default::default()
        })
    }
}

/// Per-open file operations for a network interface device node.
///
/// All state is resolved from the driver globals (`E1000_DEVICE`, `NET_STACK`)
/// at call time; the open-time DEVICE_MANAGER lookup on the inode only gates
/// access to interfaces that are actually registered.
pub struct NetDeviceFileOps;

impl FileOps for NetDeviceFileOps {
    fn read_with_flags(
        &self,
        _offset: usize,
        buf: &mut [u8],
        flags: u32,
    ) -> Result<usize, VfsError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let nonblocking = (flags & O_NONBLOCK) != 0;
        let mut frame = alloc::vec![0u8; 2048];

        loop {
            {
                let mut dev_guard = E1000_DEVICE.lock();
                let dev = dev_guard.as_mut().ok_or(VfsError::NotFound)?;
                if let Some(len) = dev.receive_packet(&mut frame).map_err(VfsError::from)? {
                    // Truncate frames larger than the caller's buffer,
                    // matching packet-socket semantics.
                    let copy_len = core::cmp::min(len, buf.len());
                    buf[..copy_len].copy_from_slice(&frame[..copy_len]);
                    return Ok(copy_len);
                }
            }

            if nonblocking {
                return Err(VfsError::WouldBlock);
            }

            // Release the device lock before yielding so other threads and the
            // network stack can make progress while we wait for frames.
            crate::proc::thread::Thread::yield_cpu();
        }
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Result<usize, VfsError> {
        if buf.is_empty() || buf.len() > BUFFER_SIZE {
            return Err(VfsError::InvalidInput);
        }

        let mut dev_guard = E1000_DEVICE.lock();
        let dev = dev_guard.as_mut().ok_or(VfsError::NotFound)?;
        dev.send_packet(buf).map_err(VfsError::from)?;
        Ok(buf.len())
    }

    fn poll_events(&self, events: i16) -> i16 {
        let mut revents = 0;

        let mut dev_guard = E1000_DEVICE.lock();
        if let Some(dev) = dev_guard.as_mut() {
            if (events & crate::syscalls::fs::POLLIN) != 0 && dev.has_pending_rx() {
                revents |= crate::syscalls::fs::POLLIN;
            }
            if (events & crate::syscalls::fs::POLLOUT) != 0 && dev.is_tx_ready() {
                revents |= crate::syscalls::fs::POLLOUT;
            }
        }
        revents
    }

    fn ioctl(&self, cmd: u64, arg: usize) -> Result<usize, VfsError> {
        let req_ptr = UserPtr::<IfReq>::from_u64(arg as u64);

        match cmd {
            SIOCGIFNAME => {
                let mut req = req_ptr.read().ok_or(VfsError::InvalidInput)?;
                // SAFETY: reading the freshly copied `ivalue` variant of the
                // ifreq union; POD read of initialized user-provided data.
                let ifindex = unsafe { req.data.ivalue };
                let names = interface_names();
                // Interface indices are 1-based, mirroring SIOCGIFINDEX output.
                if ifindex <= 0 {
                    return Err(VfsError::InvalidInput);
                }
                let name = names
                    .get((ifindex - 1) as usize)
                    .ok_or(VfsError::NotFound)?;
                req.name = name_to_bytes(name);
                req_ptr.write(req).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            SIOCGIFINDEX => {
                let mut req = req_ptr.read().ok_or(VfsError::InvalidInput)?;
                let index = resolve_index(&req.name).ok_or(VfsError::NotFound)?;
                req.set_ivalue(index as i32 + 1);
                req_ptr.write(req).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            SIOCGIFFLAGS => {
                let mut req = req_ptr.read().ok_or(VfsError::InvalidInput)?;
                resolve_index(&req.name).ok_or(VfsError::NotFound)?;

                let link_up = {
                    let mut dev_guard = E1000_DEVICE.lock();
                    match dev_guard.as_mut() {
                        Some(dev) => dev.is_link_up(),
                        None => false,
                    }
                };

                let mut flags = IFF_UP | IFF_BROADCAST | IFF_MULTICAST;
                if link_up {
                    flags |= IFF_RUNNING;
                }
                req.set_flags(flags);
                req_ptr.write(req).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            SIOCGIFHWADDR => {
                let mut req = req_ptr.read().ok_or(VfsError::InvalidInput)?;
                resolve_index(&req.name).ok_or(VfsError::NotFound)?;

                let mac = {
                    let dev_guard = E1000_DEVICE.lock();
                    match dev_guard.as_ref() {
                        Some(dev) => dev.mac_address(),
                        None => return Err(VfsError::NotFound),
                    }
                };
                req.set_hwaddr(IfSockAddr::ether(mac));
                req_ptr.write(req).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            SIOCGIFADDR | SIOCGIFDSTADDR => {
                let mut req = req_ptr.read().ok_or(VfsError::InvalidInput)?;
                resolve_index(&req.name).ok_or(VfsError::NotFound)?;

                let (octets, _) = ipv4_cidr().ok_or(VfsError::NotFound)?;
                req.set_addr(IfSockAddr::ipv4(octets));
                req_ptr.write(req).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            SIOCGIFNETMASK => {
                let mut req = req_ptr.read().ok_or(VfsError::InvalidInput)?;
                resolve_index(&req.name).ok_or(VfsError::NotFound)?;

                let (_, prefix) = ipv4_cidr().ok_or(VfsError::NotFound)?;
                req.set_addr(IfSockAddr::ipv4(prefix_to_netmask(prefix)));
                req_ptr.write(req).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            SIOCGIFBRDADDR => {
                let mut req = req_ptr.read().ok_or(VfsError::InvalidInput)?;
                resolve_index(&req.name).ok_or(VfsError::NotFound)?;

                let (octets, prefix) = ipv4_cidr().ok_or(VfsError::NotFound)?;
                let mask = prefix_to_netmask(prefix);
                let mut broadcast = [0u8; 4];
                for i in 0..4 {
                    broadcast[i] = octets[i] | !mask[i];
                }
                req.set_addr(IfSockAddr::ipv4(broadcast));
                req_ptr.write(req).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            SIOCGIFMTU => {
                let mut req = req_ptr.read().ok_or(VfsError::InvalidInput)?;
                resolve_index(&req.name).ok_or(VfsError::NotFound)?;

                req.set_ivalue(ETH_MTU as i32);
                req_ptr.write(req).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            SIOCGIFCONF => {
                let conf_ptr = UserPtr::<IfConf>::from_u64(arg as u64);
                let mut conf = conf_ptr.read().ok_or(VfsError::InvalidInput)?;

                let names = interface_names();
                let current_ipv4 = ipv4_cidr();
                let total = names.len() * core::mem::size_of::<IfReq>();

                if conf.ifc_buf == 0 {
                    // Size-query mode: report the required capacity only.
                    conf.ifc_len = total as i32;
                    conf_ptr.write(conf).ok_or(VfsError::InvalidInput)?;
                    return Ok(0);
                }

                let capacity =
                    (conf.ifc_len as usize / core::mem::size_of::<IfReq>()).min(names.len());
                let base = UserPtr::<IfReq>::from_u64(conf.ifc_buf);

                for (index, name) in names.iter().take(capacity).enumerate() {
                    let mut req = IfReq::new(name);
                    if let Some((octets, _)) = current_ipv4 {
                        req.set_addr(IfSockAddr::ipv4(octets));
                    }
                    base.add(index).write(req).ok_or(VfsError::InvalidInput)?;
                }

                conf.ifc_len = (capacity * core::mem::size_of::<IfReq>()) as i32;
                conf_ptr.write(conf).ok_or(VfsError::InvalidInput)?;
                Ok(0)
            }
            SIOCSIFADDR | SIOCSIFMTU => Err(VfsError::NotSupported),
            _ => Err(VfsError::NotSupported),
        }
    }
}

// ===== Dynamic registration =====

/// Create the inode for one network interface node under `/dev`.
fn new_net_inode(iface_name: &'static str) -> Result<Arc<Inode>, &'static str> {
    let ino = {
        let mt = MOUNT_TABLE.read();
        let (mount, _) = mt.lookup("/dev").ok_or("devfs not mounted")?;
        mount.superblock.alloc_ino()
    };

    Ok(Arc::new(Inode {
        ino,
        inode_type: InodeType::CharDevice,
        ops: Arc::new(NetDeviceInode { iface_name }),
    }))
}

/// Scan DEVICE_MANAGER for network devices and register each as `/dev/ethN`.
///
/// Runs as a late initcall so that NIC drivers (device initcalls) have already
/// probed the hardware and the network stack has been initialized.
fn register_network_devices() -> Result<(), &'static str> {
    let names = interface_names();
    for name in names {
        let inode = new_net_inode(name)?;
        crate::fs::devfs::register_dev_node(name, inode);
        log::info!("[DevFS] Registered network interface /dev/{}", name);
    }
    Ok(())
}

crate::late_initcall!(register_network_devices);
crate::MODULE_LICENSE!("BSD-2-Clause");
crate::MODULE_AUTHOR!("Ananta98");
crate::MODULE_DESCRIPTION!("Network Interface Device Filesystem Nodes");
