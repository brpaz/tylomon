use std::net::IpAddr;

/// Parsed packet information extracted from raw capture
#[derive(Debug, Clone)]
pub struct ParsedPacket {
    pub src_ip: IpAddr,
    pub dst_ip: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub tcp_flags: TcpFlags,
    pub sequence_number: u32,
    pub acknowledgment_number: u32,
    pub payload: Vec<u8>,
}

/// TCP flag bits
#[derive(Debug, Clone, Default)]
pub struct TcpFlags {
    pub syn: bool,
    pub ack: bool,
    pub fin: bool,
    pub rst: bool,
    pub psh: bool,
}

/// Parse raw packet bytes into a structured representation.
///
/// Uses etherparse for zero-copy L2-L4 parsing.
pub fn parse_packet(data: &[u8]) -> Option<ParsedPacket> {
    use etherparse::SlicedPacket;

    let packet = SlicedPacket::from_ethernet(data).ok()?;

    let (src_ip, dst_ip) = match packet.net {
        Some(etherparse::NetSlice::Ipv4(ref ipv4)) => (
            IpAddr::V4(ipv4.header().source_addr()),
            IpAddr::V4(ipv4.header().destination_addr()),
        ),
        Some(etherparse::NetSlice::Ipv6(ref ipv6)) => (
            IpAddr::V6(ipv6.header().source_addr()),
            IpAddr::V6(ipv6.header().destination_addr()),
        ),
        _ => return None,
    };

    let tcp = match packet.transport {
        Some(etherparse::TransportSlice::Tcp(ref tcp)) => tcp,
        _ => return None,
    };

    Some(ParsedPacket {
        src_ip,
        dst_ip,
        src_port: tcp.source_port(),
        dst_port: tcp.destination_port(),
        tcp_flags: TcpFlags {
            syn: tcp.syn(),
            ack: tcp.ack(),
            fin: tcp.fin(),
            rst: tcp.rst(),
            psh: tcp.psh(),
        },
        sequence_number: tcp.sequence_number(),
        acknowledgment_number: tcp.acknowledgment_number(),
        payload: tcp.payload().to_vec(),
    })
}
