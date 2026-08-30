//! Global smoltcp Network Stack Manager
//!
//! Maintains network interface state, IP routing table, socket sets, and
//! coordinates packet processing across kernel network drivers.

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::socket::AnySocket;
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpCidr, Ipv4Address, Ipv4Cidr,
    Ipv6Address, Ipv6Cidr,
};

use crate::device::{Device, DeviceType, DEVICE_MANAGER};
use crate::net::device::NetDeviceAdapter;
use crate::sync::Mutex;

/// Global network stack singleton.
pub static NET_STACK: Mutex<Option<NetworkStack>> = Mutex::new(None);

/// Return the current monotonic timestamp as a `smoltcp::time::Instant`.
pub fn current_time() -> Instant {
    let elapsed_ns = crate::arch::timer::hpet::elapsed_ns();
    Instant::from_micros((elapsed_ns / 1_000) as i64)
}

/// The core network stack state.
pub struct NetworkStack {
    pub iface: Interface,
    pub sockets: SocketSet<'static>,
    pub device: NetDeviceAdapter,
}

impl NetworkStack {
    /// Initialize the network stack with the first available network device in DEVICE_MANAGER.
    pub fn new() -> Option<Self> {
        let net_devices = DEVICE_MANAGER
            .read()
            .get_by_type(DeviceType::Network);

        let dev_arc = net_devices.first()?.clone();
        Self::with_device(dev_arc)
    }

    /// Initialize the network stack using a specific network device.
    pub fn with_device(dev_arc: Arc<Mutex<Box<dyn Device>>>) -> Option<Self> {
        let mac = {
            let mut dev_guard = dev_arc.lock();
            let net_dev = dev_guard.as_net_device_mut()?;
            net_dev.mac_address()
        };

        let ethernet_addr = EthernetAddress(mac);
        let hw_addr = HardwareAddress::Ethernet(ethernet_addr);

        let mut device = NetDeviceAdapter::new(dev_arc);
        let mut config = Config::new(hw_addr);
        config.random_seed = crate::arch::timer::hpet::elapsed_ns();

        let mut iface = Interface::new(config, &mut device, current_time());

        // Configure default IPv4 and IPv6 addresses & routes (matching QEMU user-net)
        let ipv4_addr = Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 15), 24);
        let ipv6_addr = Ipv6Cidr::new(
            Ipv6Address::new(0xfe80, 0, 0, 0, 0x5054, 0x00ff, 0xfe12, 0x3456),
            64,
        );

        iface.update_ip_addrs(|addrs| {
            let _ = addrs.push(IpCidr::Ipv4(ipv4_addr));
            let _ = addrs.push(IpCidr::Ipv6(ipv6_addr));
        });

        // Add default IPv4 gateway (10.0.2.2)
        iface.routes_mut().add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2)).ok();

        let sockets = SocketSet::new(Vec::new());

        Some(Self {
            iface,
            sockets,
            device,
        })
    }

    /// Poll the interface and all active sockets to transmit and receive queued packets.
    pub fn poll(&mut self) -> bool {
        let timestamp = current_time();
        self.iface.poll(timestamp, &mut self.device, &mut self.sockets);
        true
    }

    /// Add a socket to the global SocketSet.
    pub fn add_socket<T: AnySocket<'static>>(&mut self, socket: T) -> SocketHandle {
        self.sockets.add(socket)
    }

    /// Remove a socket from the global SocketSet.
    pub fn remove_socket(&mut self, handle: SocketHandle) {
        self.sockets.remove(handle);
    }
}

/// Poll the global network stack.
pub fn poll() {
    if let Some(ref mut stack) = *NET_STACK.lock() {
        stack.poll();
    }
}

