//! Module that contains different UDP implementations used as [ByteSource]s
use std::net::SocketAddr;

use super::ByteSource;

#[derive(Debug)]
/// Wrapper build around the a [UdpSocket](std::net::UdpSocket) used as UDP socket for the [ByteSource]
pub struct UdpReader(std::net::UdpSocket);

impl From<std::net::UdpSocket> for UdpReader {
    fn from(value: std::net::UdpSocket) -> Self {
        UdpReader(value)
    }
}

impl std::io::Read for UdpReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.recv(buf)
    }
}

#[derive(Debug)]
/// Struct used as UDP socket for the [ByteSource] that allows incoming connection only from predefined senders
pub struct CheckedUdpSocket {
    /// The [UdpSocket](std::net::UdpSocket) that received the bytestream
    socket: std::net::UdpSocket,
    /// The allowed senders
    allowed_sender: Vec<SocketAddr>,
}

impl CheckedUdpSocket {
    /// Creates a new [CheckedUdpSocket] form a socket and the allowed sender addresses
    pub fn new(socket: std::net::UdpSocket, allowed_sender: Vec<SocketAddr>) -> Self {
        Self {
            socket,
            allowed_sender,
        }
    }
}

impl ByteSource for CheckedUdpSocket {
    type Error = CheckUdpError;

    fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error> {
        match self.socket.recv_from(buffer) {
            Ok((package_size, sender)) if self.allowed_sender.contains(&sender) => {
                Ok(Some(package_size))
            }
            Ok((_, sender)) => Err(CheckUdpError::InvalidSender(sender)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Debug)]
/// Error used in the implementation of the [ByteSource] for [CheckedUdpSocket]
pub enum CheckUdpError {
    /// Error to indicate that the receiving of the bytestream resulted in an error
    IOError(std::io::Error),
    /// Error to indicate that bytestream was received from an invalid sender
    InvalidSender(SocketAddr),
}

impl std::fmt::Display for CheckUdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckUdpError::IOError(e) => write!(f, "{e}"),
            CheckUdpError::InvalidSender(sender) => write!(f, "Received package from {sender}"),
        }
    }
}

impl std::error::Error for CheckUdpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CheckUdpError::IOError(e) => Some(e),
            CheckUdpError::InvalidSender(_) => None,
        }
    }
}

impl From<std::io::Error> for CheckUdpError {
    fn from(value: std::io::Error) -> Self {
        CheckUdpError::IOError(value)
    }
}
