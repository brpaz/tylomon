use std::collections::HashMap;
use std::net::IpAddr;

use crate::capture::packet::ParsedPacket;

/// Unique identifier for a TCP flow (connection)
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct FlowKey {
    pub src_ip: IpAddr,
    pub src_port: u16,
    pub dst_ip: IpAddr,
    pub dst_port: u16,
}

impl FlowKey {
    pub fn from_packet(packet: &ParsedPacket) -> Self {
        Self {
            src_ip: packet.src_ip,
            src_port: packet.src_port,
            dst_ip: packet.dst_ip,
            dst_port: packet.dst_port,
        }
    }

    /// Get the reverse direction flow key
    pub fn reverse(&self) -> Self {
        Self {
            src_ip: self.dst_ip,
            src_port: self.dst_port,
            dst_ip: self.src_ip,
            dst_port: self.src_port,
        }
    }
}

/// Reassembles TCP stream data from individual packets
pub struct StreamReassembler {
    /// Active streams indexed by flow key
    streams: HashMap<FlowKey, StreamBuffer>,
}

/// Buffer for a single TCP stream direction
struct StreamBuffer {
    /// Accumulated payload bytes (assumes in-order delivery)
    data: Vec<u8>,
    /// Next expected sequence number
    next_seq: Option<u32>,
}

impl Default for StreamReassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamReassembler {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
        }
    }

    /// Add a packet to the reassembler.
    /// Returns accumulated data if the stream has new complete data.
    pub fn process_packet(&mut self, packet: &ParsedPacket) -> Option<StreamData> {
        let key = FlowKey::from_packet(packet);

        // Handle connection lifecycle
        if packet.tcp_flags.syn {
            self.streams.insert(
                key.clone(),
                StreamBuffer {
                    data: Vec::new(),
                    next_seq: Some(packet.sequence_number.wrapping_add(1)),
                },
            );
            return None;
        }

        if packet.tcp_flags.rst || packet.tcp_flags.fin {
            if let Some(buffer) = self.streams.remove(&key)
                && !buffer.data.is_empty()
            {
                return Some(StreamData {
                    flow: key,
                    data: buffer.data,
                    complete: true,
                });
            }
            return None;
        }

        // Append payload data
        if !packet.payload.is_empty() {
            let buffer = self.streams.entry(key.clone()).or_insert(StreamBuffer {
                data: Vec::new(),
                next_seq: Some(packet.sequence_number),
            });

            buffer.data.extend_from_slice(&packet.payload);
            buffer.next_seq = Some(
                packet
                    .sequence_number
                    .wrapping_add(packet.payload.len() as u32),
            );

            return Some(StreamData {
                flow: key,
                data: buffer.data.clone(),
                complete: false,
            });
        }

        None
    }

    pub fn cleanup_stale(&mut self, max_streams: usize) {
        while self.streams.len() > max_streams {
            if let Some(key) = self.streams.keys().next().cloned() {
                self.streams.remove(&key);
            }
        }
    }

    pub fn drain(&mut self, flow: &FlowKey, count: usize) {
        if let Some(buffer) = self.streams.get_mut(flow) {
            if count >= buffer.data.len() {
                buffer.data.clear();
            } else {
                buffer.data.drain(..count);
            }
        }
    }
}

/// Data accumulated from a TCP stream
#[derive(Debug)]
pub struct StreamData {
    pub flow: FlowKey,
    pub data: Vec<u8>,
    pub complete: bool,
}
