//! This plugins enables the parsing of network packets either from a pcap file or from a network device.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::net::IpAddr;
use std::num::TryFromIntError;
use std::path::PathBuf;
use std::str::FromStr;

use etherparse::{
    Ethernet2Header, InternetSlice, Ipv4Header, Ipv6Header, LinkSlice, SlicedPacket, TcpHeader, TransportSlice,
    UdpHeader,
};
use ip_network::IpNetwork;
use pcap::{Activated, Capture, Device, Error as PCAPError, Packet};
use rtlola_interpreter::input::{EventFactoryError, InputMap, ValueGetter};
use rtlola_interpreter::time::TimeRepresentation;
use rtlola_interpreter::Value;

use crate::EventSource;

// ################################
// Packet parsing functions
// ################################

//ethernet functions
fn get_ethernet_header(packet: &SlicedPacket) -> Option<Ethernet2Header> {
    match &packet.link {
        Some(s) => {
            match s {
                LinkSlice::Ethernet2(hdr) => Some(hdr.to_header()),
            }
        },
        None => None,
    }
}

fn ethernet_source(packet: &SlicedPacket) -> Value {
    if let Some(ethernet_header) = get_ethernet_header(packet) {
        let values: Vec<Value> = ethernet_header
            .source
            .iter()
            .map(|v: &u8| Value::Unsigned((*v).into()))
            .collect();
        Value::Tuple(values.into_boxed_slice())
    } else {
        Value::None
    }
}
fn ethernet_destination(packet: &SlicedPacket) -> Value {
    if let Some(ethernet_header) = get_ethernet_header(packet) {
        let values: Vec<Value> = ethernet_header
            .destination
            .iter()
            .map(|v: &u8| Value::Unsigned((*v).into()))
            .collect();
        Value::Tuple(values.into_boxed_slice())
    } else {
        Value::None
    }
}
fn ethernet_type(packet: &SlicedPacket) -> Value {
    if let Some(ethernet_header) = get_ethernet_header(packet) {
        Value::Unsigned(ethernet_header.ether_type.into())
    } else {
        Value::None
    }
}

//Ipv4 functions
fn get_ipv4_header(packet: &SlicedPacket) -> Option<Ipv4Header> {
    match &packet.ip {
        Some(int_slice) => {
            match int_slice {
                InternetSlice::Ipv4(h, _) => Some(h.to_header()),
                InternetSlice::Ipv6(_, _) => None,
            }
        },
        None => None,
    }
}

fn ipv4_source(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        let values: Vec<Value> = header
            .source
            .iter()
            .map(|v: &u8| Value::Unsigned((*v).into()))
            .collect();
        Value::Tuple(values.into_boxed_slice())
    } else {
        Value::None
    }
}
fn ipv4_destination(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        let values: Vec<Value> = header
            .destination
            .iter()
            .map(|v: &u8| Value::Unsigned((*v).into()))
            .collect();
        Value::Tuple(values.into_boxed_slice())
    } else {
        Value::None
    }
}
fn ipv4_ihl(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Unsigned(header.ihl().into())
    } else {
        Value::None
    }
}
fn ipv4_dscp(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Unsigned(header.differentiated_services_code_point.into())
    } else {
        Value::None
    }
}
fn ipv4_ecn(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Unsigned(header.explicit_congestion_notification.into())
    } else {
        Value::None
    }
}
fn ipv4_length(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Unsigned(header.total_len().into())
    } else {
        Value::None
    }
}
fn ipv4_id(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Unsigned(header.identification.into())
    } else {
        Value::None
    }
}
fn ipv4_fragment_offset(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Unsigned(header.fragments_offset.into())
    } else {
        Value::None
    }
}
fn ipv4_ttl(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Unsigned(header.time_to_live.into())
    } else {
        Value::None
    }
}
fn ipv4_protocol(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Unsigned(header.protocol.into())
    } else {
        Value::None
    }
}
fn ipv4_checksum(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Unsigned(header.header_checksum.into())
    } else {
        Value::None
    }
}
fn ipv4_flags_df(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Bool(header.dont_fragment)
    } else {
        Value::None
    }
}
fn ipv4_flags_mf(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv4_header(packet) {
        Value::Bool(header.more_fragments)
    } else {
        Value::None
    }
}

//IPv6 functions
fn get_ipv6_header(packet: &SlicedPacket) -> Option<Ipv6Header> {
    match &packet.ip {
        Some(int_slice) => {
            match int_slice {
                InternetSlice::Ipv4(_, _) => None,
                InternetSlice::Ipv6(h, _) => Some(h.to_header()),
            }
        },
        None => None,
    }
}

fn ipv6_source(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv6_header(packet) {
        let values: Vec<Value> = header
            .source
            .iter()
            .map(|v: &u8| Value::Unsigned((*v).into()))
            .collect();
        Value::Tuple(values.into_boxed_slice())
    } else {
        Value::None
    }
}
fn ipv6_destination(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv6_header(packet) {
        let values: Vec<Value> = header
            .destination
            .iter()
            .map(|v: &u8| Value::Unsigned((*v).into()))
            .collect();
        Value::Tuple(values.into_boxed_slice())
    } else {
        Value::None
    }
}
fn ipv6_traffic_class(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv6_header(packet) {
        Value::Unsigned(header.traffic_class.into())
    } else {
        Value::None
    }
}
fn ipv6_flow_label(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv6_header(packet) {
        Value::Unsigned(header.flow_label.into())
    } else {
        Value::None
    }
}
fn ipv6_length(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv6_header(packet) {
        Value::Unsigned(header.payload_length.into())
    } else {
        Value::None
    }
}
fn ipv6_hop_limit(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_ipv6_header(packet) {
        Value::Unsigned(header.hop_limit.into())
    } else {
        Value::None
    }
}

//TCP functions
fn get_tcp_header(packet: &SlicedPacket) -> Option<TcpHeader> {
    use TransportSlice::*;
    match &packet.transport {
        Some(t) => {
            match t {
                Tcp(h) => Some(h.to_header()),
                Udp(_) | Icmpv4(_) | Icmpv6(_) | Unknown(_) => None,
            }
        },
        None => None,
    }
}

fn tcp_source_port(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Unsigned(header.source_port.into())
    } else {
        Value::None
    }
}
fn tcp_destination_port(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Unsigned(header.destination_port.into())
    } else {
        Value::None
    }
}
fn tcp_seq_number(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Unsigned(header.sequence_number.into())
    } else {
        Value::None
    }
}
fn tcp_ack_number(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Unsigned(header.acknowledgment_number.into())
    } else {
        Value::None
    }
}
fn tcp_data_offset(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Unsigned(header.data_offset().into())
    } else {
        Value::None
    }
}
fn tcp_flags_ns(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Bool(header.ns)
    } else {
        Value::None
    }
}
fn tcp_flags_fin(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Bool(header.fin)
    } else {
        Value::None
    }
}
fn tcp_flags_syn(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Bool(header.syn)
    } else {
        Value::None
    }
}
fn tcp_flags_rst(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Bool(header.rst)
    } else {
        Value::None
    }
}
fn tcp_flags_psh(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Bool(header.psh)
    } else {
        Value::None
    }
}
fn tcp_flags_ack(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Bool(header.ack)
    } else {
        Value::None
    }
}
fn tcp_flags_urg(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Bool(header.urg)
    } else {
        Value::None
    }
}
fn tcp_flags_ece(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Bool(header.ece)
    } else {
        Value::None
    }
}
fn tcp_flags_cwr(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Bool(header.cwr)
    } else {
        Value::None
    }
}
fn tcp_window_size(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Unsigned(header.window_size.into())
    } else {
        Value::None
    }
}
fn tcp_checksum(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Unsigned(header.checksum.into())
    } else {
        Value::None
    }
}
fn tcp_urgent_pointer(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_tcp_header(packet) {
        Value::Unsigned(header.urgent_pointer.into())
    } else {
        Value::None
    }
}

//UDP functions
fn get_udp_header(packet: &SlicedPacket) -> Option<UdpHeader> {
    use TransportSlice::*;
    match &packet.transport {
        Some(t) => {
            match t {
                Tcp(_) | Icmpv4(_) | Icmpv6(_) | Unknown(_) => None,
                Udp(h) => Some(h.to_header()),
            }
        },
        None => None,
    }
}
fn udp_source_port(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_udp_header(packet) {
        Value::Unsigned(header.source_port.into())
    } else {
        Value::None
    }
}
fn udp_destination_port(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_udp_header(packet) {
        Value::Unsigned(header.destination_port.into())
    } else {
        Value::None
    }
}
fn udp_length(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_udp_header(packet) {
        Value::Unsigned(header.length.into())
    } else {
        Value::None
    }
}
fn udp_checksum(packet: &SlicedPacket) -> Value {
    if let Some(header) = get_udp_header(packet) {
        Value::Unsigned(header.checksum.into())
    } else {
        Value::None
    }
}

//Misc Packet Functions
fn get_packet_payload(packet: &SlicedPacket) -> Value {
    Value::Str(String::from_utf8_lossy(packet.payload).as_ref().into())
}

fn get_packet_protocol(packet: &SlicedPacket) -> Value {
    match &packet.transport {
        Some(transport) => {
            match transport {
                TransportSlice::Tcp(_) => Value::Str("TCP".into()),
                TransportSlice::Udp(_) => Value::Str("UDP".into()),
                TransportSlice::Icmpv4(_) => Value::Str("ICMPv4".into()),
                TransportSlice::Icmpv6(_) => Value::Str("ICMPv6".into()),
                TransportSlice::Unknown(_) => Value::Str("Unknown".into()),
            }
        },
        None => {
            match &packet.ip {
                Some(ip) => {
                    match ip {
                        InternetSlice::Ipv4(_, _) => Value::Str("IPv4".into()),
                        InternetSlice::Ipv6(_, _) => Value::Str("IPv6".into()),
                    }
                },
                None => {
                    match &packet.link {
                        Some(link) => {
                            match link {
                                LinkSlice::Ethernet2(_) => Value::Str("Ethernet2".into()),
                            }
                        },
                        None => Value::Str("Unknown".into()),
                    }
                },
            }
        },
    }
}

/// Represents different kind of errors that might occur.
#[derive(Debug)]
pub enum PcapError {
    UnknownInput(String),
    UnknownDevice(String, Vec<String>),
    InvalidLocalNetwork(String, ip_network::IpNetworkParseError),
    TimeParseError(TryFromIntError),
    TimeFormatError(String),
    Pcap(pcap::Error),
}

impl Display for PcapError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PcapError::UnknownInput(i) => write!(f, "Malformed input name: {}", i),
            PcapError::UnknownDevice(d, available) => {
                write!(
                    f,
                    "Interface {} was not found. Available interfaces are: {}",
                    d,
                    available.join(", ")
                )
            },
            PcapError::InvalidLocalNetwork(name, e) => {
                write!(f, "Could not parse local network range: {}. Error: {}", *name, e)
            },
            PcapError::TimeParseError(e) => write!(f, "Could not parse timestamp from packet: {}", e),
            PcapError::TimeFormatError(e) => write!(f, "Could not parse timestamp to time format: {}", e),
            PcapError::Pcap(e) => write!(f, "Could not process packet: {}", e),
        }
    }
}

impl Error for PcapError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PcapError::UnknownInput(_) => None,
            PcapError::UnknownDevice(_, _) => None,
            PcapError::InvalidLocalNetwork(_, e) => Some(e),
            PcapError::TimeParseError(e) => Some(e),
            PcapError::TimeFormatError(_) => None,
            PcapError::Pcap(e) => Some(e),
        }
    }
}

impl From<PcapError> for EventFactoryError {
    fn from(value: PcapError) -> Self {
        match value {
            PcapError::UnknownInput(i) => EventFactoryError::InputStreamUnknown(vec![i]),
            e => EventFactoryError::Other(Box::new(e)),
        }
    }
}

/// Represents a raw network packet
pub struct PcapRecord(Vec<u8>);

impl InputMap for PcapRecord {
    type CreationData = IpNetwork;
    type Error = PcapError;

    fn func_for_input(name: &str, data: Self::CreationData) -> Result<ValueGetter<Self, PcapError>, PcapError> {
        let local_net = data;
        let layers: Vec<&str> = name.split("::").collect();
        if layers.len() > 3 || layers.is_empty() {
            return Err(PcapError::UnknownInput(name.to_string()));
        }

        let get_packet_direction = move |packet: &SlicedPacket| -> Value {
            let addr: IpAddr = match &packet.ip {
                Some(ip) => {
                    match ip {
                        InternetSlice::Ipv4(header, _) => IpAddr::V4(header.destination_addr()),
                        InternetSlice::Ipv6(header, _) => IpAddr::V6(header.destination_addr()),
                    }
                },
                None => return Value::None,
            };
            if local_net.contains(addr) {
                Value::Str("Incoming".into())
            } else {
                Value::Str("Outgoing".into())
            }
        };

        let inner_fn: Box<dyn Fn(&SlicedPacket) -> Value> = match layers[0] {
            "Ethernet" => {
                if layers.len() != 2 {
                    return Err(PcapError::UnknownInput(name.to_string()));
                };
                match layers[1] {
                    "source" => Box::new(ethernet_source),
                    "destination" => Box::new(ethernet_destination),
                    "type" => Box::new(ethernet_type),
                    _ => {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    },
                }
            },
            "IPv4" => {
                if layers.len() < 2 {
                    return Err(PcapError::UnknownInput(name.to_string()));
                };
                match layers[1] {
                    "source" => Box::new(ipv4_source),
                    "destination" => Box::new(ipv4_destination),
                    "ihl" => Box::new(ipv4_ihl),
                    "dscp" => Box::new(ipv4_dscp),
                    "ecn" => Box::new(ipv4_ecn),
                    "length" => Box::new(ipv4_length),
                    "identification" => Box::new(ipv4_id),
                    "fragment_offset" => Box::new(ipv4_fragment_offset),
                    "ttl" => Box::new(ipv4_ttl),
                    "protocol" => Box::new(ipv4_protocol),
                    "checksum" => Box::new(ipv4_checksum),
                    "flags" => {
                        if layers.len() < 3 {
                            return Err(PcapError::UnknownInput(name.to_string()));
                        }
                        match layers[2] {
                            "df" => Box::new(ipv4_flags_df),
                            "mf" => Box::new(ipv4_flags_mf),
                            _ => {
                                return Err(PcapError::UnknownInput(name.to_string()));
                            },
                        }
                    },
                    _ => {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    },
                }
            },
            "IPv6" => {
                if layers.len() < 2 {
                    return Err(PcapError::UnknownInput(name.to_string()));
                };
                match layers[1] {
                    "source" => Box::new(ipv6_source),
                    "destination" => Box::new(ipv6_destination),
                    "traffic_class" => Box::new(ipv6_traffic_class),
                    "flow_label" => Box::new(ipv6_flow_label),
                    "length" => Box::new(ipv6_length),
                    "hop_limit" => Box::new(ipv6_hop_limit),
                    _ => {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    },
                }
            },
            //"ICMP" => {},
            "TCP" => {
                if layers.len() < 2 {
                    return Err(PcapError::UnknownInput(name.to_string()));
                };
                match layers[1] {
                    "source" => Box::new(tcp_source_port),
                    "destination" => Box::new(tcp_destination_port),
                    "seq_number" => Box::new(tcp_seq_number),
                    "ack_number" => Box::new(tcp_ack_number),
                    "data_offset" => Box::new(tcp_data_offset),
                    "window_size" => Box::new(tcp_window_size),
                    "checksum" => Box::new(tcp_checksum),
                    "urgent_pointer" => Box::new(tcp_urgent_pointer),
                    "flags" => {
                        if layers.len() < 3 {
                            return Err(PcapError::UnknownInput(name.to_string()));
                        };
                        match layers[2] {
                            "ns" => Box::new(tcp_flags_ns),
                            "fin" => Box::new(tcp_flags_fin),
                            "syn" => Box::new(tcp_flags_syn),
                            "rst" => Box::new(tcp_flags_rst),
                            "psh" => Box::new(tcp_flags_psh),
                            "ack" => Box::new(tcp_flags_ack),
                            "urg" => Box::new(tcp_flags_urg),
                            "ece" => Box::new(tcp_flags_ece),
                            "cwr" => Box::new(tcp_flags_cwr),
                            _ => {
                                return Err(PcapError::UnknownInput(name.to_string()));
                            },
                        }
                    },
                    _ => {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    },
                }
            },
            "UDP" => {
                if layers.len() < 2 {
                    return Err(PcapError::UnknownInput(name.to_string()));
                };
                match layers[1] {
                    "source" => Box::new(udp_source_port),
                    "destination" => Box::new(udp_destination_port),
                    "length" => Box::new(udp_length),
                    "checksum" => Box::new(udp_checksum),
                    _ => {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    },
                }
            },
            "payload" => Box::new(get_packet_payload),
            "direction" => Box::new(get_packet_direction),
            "protocol" => Box::new(get_packet_protocol),
            _ => {
                return Err(PcapError::UnknownInput(name.to_string()));
            },
        };
        Ok(Box::new(move |packet: &PcapRecord| {
            let packet = SlicedPacket::from_ethernet(packet.0.as_slice()).expect("Could not parse packet!");
            Ok(inner_fn(&packet))
        }))
    }
}

// ################################
// Event Source Handling
// ################################
/// Determines the input source for network packets.
#[derive(Debug, Clone)]
pub enum PcapInputSource {
    /// Use the specified device for packet input.
    Device {
        /// The name of the device to use.
        name: String,
        /// The description of your local network IP address range in CIDR-Notation.
        local_network: String,
    },
    /// Use the given file for packet input.
    File {
        /// The path to the PCAP file.
        path: PathBuf,
        /// The description of your local network IP address range in CIDR-Notation.
        local_network: String,
    },
}

/// Parses events from network packets.
#[allow(missing_debug_implementations)] // Capture -> PcapOnDemand does not implement Debug.
pub struct PcapEventSource<InputTime: TimeRepresentation> {
    capture_handle: Capture<dyn Activated>,
    timer: PhantomData<InputTime>,
    local_net: IpNetwork,
}

impl<InputTime: TimeRepresentation> PcapEventSource<InputTime> {
    pub fn setup(src: &PcapInputSource) -> Result<PcapEventSource<InputTime>, PcapError> {
        let capture_handle = match src {
            PcapInputSource::Device { name, .. } => {
                let all_devices = Device::list().map_err(PcapError::Pcap)?;
                let device_names: Vec<_> = all_devices.iter().map(|d| d.name.clone()).collect();
                let dev: Device = all_devices
                    .into_iter()
                    .find(|d| &d.name == name)
                    .ok_or_else(|| PcapError::UnknownDevice(name.clone(), device_names))?;

                let capture_handle = Capture::from_device(dev)
                    .map_err(PcapError::Pcap)?
                    .promisc(true)
                    .snaplen(65535)
                    .open()
                    .map_err(PcapError::Pcap)?;
                capture_handle.into()
            },
            PcapInputSource::File { path, .. } => {
                let capture_handle = Capture::from_file(path).map_err(PcapError::Pcap)?;
                capture_handle.into()
            },
        };

        let local_network_range = match src {
            PcapInputSource::Device { local_network, .. } => local_network,
            PcapInputSource::File { local_network, .. } => local_network,
        };
        let local_net = IpNetwork::from_str(local_network_range.as_ref())
            .map_err(|e| PcapError::InvalidLocalNetwork(local_network_range.clone(), e))?;

        Ok(Self {
            capture_handle,
            timer: Default::default(),
            local_net,
        })
    }
}

impl<InputTime: TimeRepresentation> EventSource<InputTime> for PcapEventSource<InputTime> {
    type Error = PcapError;
    type Rec = PcapRecord;

    fn init_data(&self) -> Result<IpNetwork, PcapError> {
        Ok(self.local_net)
    }

    fn next_event(&mut self) -> Result<Option<(PcapRecord, InputTime::InnerTime)>, PcapError> {
        let raw_packet: Packet = match self.capture_handle.next_packet() {
            Ok(pkt) => pkt,
            Err(PCAPError::NoMorePackets) => return Ok(None),
            Err(e) => return Err(PcapError::Pcap(e)),
        };
        let p = (*raw_packet).to_vec();

        let (secs, nanos) = u64::try_from(raw_packet.header.ts.tv_sec)
            .and_then(|secs| u32::try_from(raw_packet.header.ts.tv_usec * 1000).map(|sub_secs| (secs, sub_secs)))
            .map_err(PcapError::TimeParseError)?;
        let time_str = format!("{}.{:09}", secs, nanos);
        let time = InputTime::parse(&time_str).map_err(PcapError::TimeFormatError)?;

        Ok(Some((PcapRecord(p), time)))
    }
}
