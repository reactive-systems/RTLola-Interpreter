use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub(crate) mod structs;
mod tcp;
mod udp;

fn socket_addr() -> (SocketAddr, SocketAddr) {
    let receiver_port = portpicker::pick_unused_port().expect("No ports free");
    let receiver_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), receiver_port);
    let sender_port = portpicker::pick_unused_port().expect("No ports free");
    let sender_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), sender_port);
    (receiver_addr, sender_addr)
}
