//! Networking stack subsystem leveraging smoltcp.
//! Enforces safety guidelines and denies unsafe code.

pub mod device;
pub mod socket;

pub use device::{SmoltcpDevice, SmoltcpRxToken, SmoltcpTxToken};

use crate::drivers::net::DEFAULT_NET_DEVICE;
use crate::drivers::timer::Timer;
use crate::drivers::timer::Tsc;
use alloc::boxed::Box;
use ostd::sync::SpinLock;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::socket::dhcpv4::Socket as Dhcpv4Socket;
use smoltcp::socket::dns::{DnsQuery, Socket as DnsSocket};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr};

pub struct NetStack {
    pub interface: Interface,
    pub sockets: SocketSet<'static>,
    pub dhcp_handle: SocketHandle,
    pub dns_handle: SocketHandle,
}

pub static NET_STACK: SpinLock<Option<NetStack>> = SpinLock::new(None);

/// Initialize the global smoltcp network stack.
pub fn init() {
    let now_ns = Tsc::new().current_time_ns();
    let timestamp = Instant::from_millis((now_ns / 1_000_000) as i64);

    if let Some(device_arc) = DEFAULT_NET_DEVICE.get() {
        let mut dev = SmoltcpDevice::new(device_arc.clone());
        let mac = device_arc.mac_address();
        let ethernet_addr = EthernetAddress::from_bytes(&mac);

        let mut config = Config::new(HardwareAddress::Ethernet(ethernet_addr));
        config.random_seed = 0x12345678;

        let mut interface = Interface::new(config, &mut dev, timestamp);

        // Bootstrap with a static IP; DHCP will overwrite this.
        let ip_addr = IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24);
        interface.update_ip_addrs(|addrs| {
            let _ = addrs.push(ip_addr);
        });

        let mut sockets = SocketSet::new(alloc::vec![]);

        // ── DHCPv4 client ──────────────────────────────────────────────
        // Leak a receive buffer so the socket can live for 'static.
        let dhcp_buf: &'static mut [u8] = Box::leak(alloc::vec![0u8; 1500].into_boxed_slice());
        let mut dhcp_socket = Dhcpv4Socket::new();
        dhcp_socket.set_receive_packet_buffer(dhcp_buf);
        let dhcp_handle = sockets.add(dhcp_socket);

        // ── DNS client ────────────────────────────────────────────────
        let dns_queries: &'static mut [Option<DnsQuery>] = Box::leak(Box::new([None, None, None]));
        let dns_socket = DnsSocket::new(&[], &mut dns_queries[..]);
        let dns_handle = sockets.add(dns_socket);

        let stack = NetStack {
            interface,
            sockets,
            dhcp_handle,
            dns_handle,
        };

        NET_STACK.lock().replace(stack);
    }
}

/// Poll the global network stack to process packets.
/// Handles DHCP configuration updates and ICMP echo requests automatically.
pub fn poll() {
    let mut stack_guard = NET_STACK.lock();
    if let Some(stack) = stack_guard.as_mut() {
        if let Some(device_arc) = DEFAULT_NET_DEVICE.get() {
            let mut dev = SmoltcpDevice::new(device_arc.clone());
            let now_ns = Tsc::new().current_time_ns();
            let timestamp = Instant::from_millis((now_ns / 1_000_000) as i64);
            let _ = stack
                .interface
                .poll(timestamp, &mut dev, &mut stack.sockets);

            // ── Apply DHCP configuration changes ──────────────────────
            let mut new_cidr = None;
            let mut new_dns = alloc::vec::Vec::new();
            {
                let dhcp = stack.sockets.get_mut::<Dhcpv4Socket>(stack.dhcp_handle);
                while let Some(event) = dhcp.poll() {
                    use smoltcp::socket::dhcpv4::Event;
                    match event {
                        Event::Configured(config) => {
                            new_cidr = Some(config.address);
                            for &srv in config.dns_servers.iter() {
                                new_dns.push(IpAddress::from(srv));
                            }
                        }
                        Event::Deconfigured => {
                            log::warn!("DHCP lease lost");
                        }
                    }
                }
            }
            if let Some(cidr) = new_cidr {
                stack.interface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    let _ = addrs.push(IpCidr::Ipv4(cidr));
                });
            }
            if !new_dns.is_empty() {
                let dns = stack.sockets.get_mut::<DnsSocket>(stack.dns_handle);
                dns.update_servers(&new_dns);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(ktest)]
mod tests {
    use super::*;
    use crate::drivers::net::NetDevice;
    use alloc::sync::Arc;
    use ostd::prelude::ktest;
    use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer as TcpSocketBuffer};
    use smoltcp::socket::udp::{
        PacketBuffer as UdpPacketBuffer, PacketMetadata as UdpPacketMetadata, Socket as UdpSocket,
    };
    use smoltcp::wire::IpEndpoint;

    #[ktest]
    fn test_udp_loopback() {
        let sim_device = Arc::new(crate::drivers::net::SimulatedNetDevice::new());
        let mut dev = SmoltcpDevice::new(sim_device.clone());

        let mac = sim_device.mac_address();
        let ethernet_addr = EthernetAddress::from_bytes(&mac);

        let mut config = Config::new(HardwareAddress::Ethernet(ethernet_addr));
        config.random_seed = 0x12345678;

        let now_ns = Tsc::new().current_time_ns();
        let timestamp = Instant::from_millis((now_ns / 1_000_000) as i64);

        let mut interface = Interface::new(config, &mut dev, timestamp);

        let ip_addr = IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24);
        interface.update_ip_addrs(|addrs| {
            let _ = addrs.push(ip_addr);
        });

        let mut sockets = SocketSet::new(alloc::vec![]);

        // Create socket 1: Receiver (bound to port 1234)
        let rx_meta = alloc::vec![UdpPacketMetadata::EMPTY; 4];
        let rx_data = alloc::vec![0u8; 1024];
        let tx_meta = alloc::vec![UdpPacketMetadata::EMPTY; 4];
        let tx_data = alloc::vec![0u8; 1024];
        let rx_buffer = UdpPacketBuffer::new(rx_meta, rx_data);
        let tx_buffer = UdpPacketBuffer::new(tx_meta, tx_data);
        let mut socket1 = UdpSocket::new(rx_buffer, tx_buffer);
        socket1.bind(1234).unwrap();
        let handle1 = sockets.add(socket1);

        // Create socket 2: Sender (bound to port 5678)
        let rx_meta2 = alloc::vec![UdpPacketMetadata::EMPTY; 4];
        let rx_data2 = alloc::vec![0u8; 1024];
        let tx_meta2 = alloc::vec![UdpPacketMetadata::EMPTY; 4];
        let tx_data2 = alloc::vec![0u8; 1024];
        let rx_buffer2 = UdpPacketBuffer::new(rx_meta2, rx_data2);
        let tx_buffer2 = UdpPacketBuffer::new(tx_meta2, tx_data2);
        let mut socket2 = UdpSocket::new(rx_buffer2, tx_buffer2);
        socket2.bind(5678).unwrap();
        let handle2 = sockets.add(socket2);

        // Send a UDP packet from socket 2 to socket 1
        {
            let socket: &mut UdpSocket = sockets.get_mut(handle2);
            let dest_endpoint = IpEndpoint::new(IpAddress::v4(10, 0, 2, 15), 1234);
            socket.send_slice(b"hello network", dest_endpoint).unwrap();
        }

        // Poll interface to transmit the packet
        let now_ns = Tsc::new().current_time_ns();
        let timestamp = Instant::from_millis((now_ns / 1_000_000) as i64);
        interface.poll(timestamp, &mut dev, &mut sockets);

        // Poll interface to process the received packet
        let now_ns = Tsc::new().current_time_ns();
        let timestamp = Instant::from_millis((now_ns / 1_000_000) as i64);
        interface.poll(timestamp, &mut dev, &mut sockets);

        // Verify socket 1 received the packet
        {
            let socket: &mut UdpSocket = sockets.get_mut(handle1);
            let mut buf = [0u8; 32];
            let (len, sender) = socket.recv_slice(&mut buf).unwrap();
            assert_eq!(len, 13);
            assert_eq!(&buf[..len], b"hello network");
            assert_eq!(sender.endpoint.port, 5678);
        }
    }

    #[ktest]
    fn test_tcp_loopback() {
        let sim_device = Arc::new(crate::drivers::net::SimulatedNetDevice::new());
        let mut dev = SmoltcpDevice::new(sim_device.clone());

        let mac = sim_device.mac_address();
        let ethernet_addr = EthernetAddress::from_bytes(&mac);

        let mut config = Config::new(HardwareAddress::Ethernet(ethernet_addr));
        config.random_seed = 0x12345678;

        let now_ns = Tsc::new().current_time_ns();
        let timestamp = Instant::from_millis((now_ns / 1_000_000) as i64);

        let mut interface = Interface::new(config, &mut dev, timestamp);

        let ip_addr = IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24);
        interface.update_ip_addrs(|addrs| {
            let _ = addrs.push(ip_addr);
        });

        let mut sockets = SocketSet::new(alloc::vec![]);

        // Create TCP Listener (port 80)
        let rx_data = alloc::vec![0u8; 1024];
        let tx_data = alloc::vec![0u8; 1024];
        let rx_buffer = TcpSocketBuffer::new(rx_data);
        let tx_buffer = TcpSocketBuffer::new(tx_data);
        let mut server_socket = TcpSocket::new(rx_buffer, tx_buffer);
        server_socket.listen(80).unwrap();
        let server_handle = sockets.add(server_socket);

        // Create TCP Client
        let rx_data2 = alloc::vec![0u8; 1024];
        let tx_data2 = alloc::vec![0u8; 1024];
        let rx_buffer2 = TcpSocketBuffer::new(rx_data2);
        let tx_buffer2 = TcpSocketBuffer::new(tx_data2);
        let client_socket = TcpSocket::new(rx_buffer2, tx_buffer2);

        let client_handle = sockets.add(client_socket);

        // Connect client to server
        {
            let client: &mut TcpSocket = sockets.get_mut(client_handle);
            client
                .connect(
                    interface.context(),
                    IpEndpoint::new(IpAddress::v4(10, 0, 2, 15), 80),
                    IpEndpoint::new(IpAddress::v4(10, 0, 2, 15), 45678),
                )
                .unwrap();
        }

        // Poll multiple times to complete connection handshake (SYN -> SYN-ACK -> ACK)
        for _ in 0..5 {
            let now_ns = Tsc::new().current_time_ns();
            let timestamp = Instant::from_millis((now_ns / 1_000_000) as i64);
            interface.poll(timestamp, &mut dev, &mut sockets);
        }

        // Check if server accepted the connection
        {
            let server: &mut TcpSocket = sockets.get_mut(server_handle);
            assert!(server.is_active());
        }

        // Send data from client
        {
            let client: &mut TcpSocket = sockets.get_mut(client_handle);
            client.send_slice(b"tcp test data").unwrap();
        }

        // Poll to transmit data
        for _ in 0..3 {
            let now_ns = Tsc::new().current_time_ns();
            let timestamp = Instant::from_millis((now_ns / 1_000_000) as i64);
            interface.poll(timestamp, &mut dev, &mut sockets);
        }

        // Read data on server
        {
            let server: &mut TcpSocket = sockets.get_mut(server_handle);
            let mut buf = [0u8; 32];
            let len = server.recv_slice(&mut buf).unwrap();
            assert_eq!(len, 13);
            assert_eq!(&buf[..len], b"tcp test data");
        }
    }
}
