use std::net::SocketAddr;
use std::usize;

use super::NetworkSource;

#[derive(Debug)]
pub struct UdpWrapper(std::net::UdpSocket);

impl From<std::net::UdpSocket> for UdpWrapper {
    fn from(value: std::net::UdpSocket) -> Self {
        UdpWrapper(value)
    }
}

impl std::io::Read for UdpWrapper {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.recv(buf)
    }
}

#[derive(Debug)]
pub struct CheckedUdpSocket {
    socket: std::net::UdpSocket,
    allowed_sender: Vec<SocketAddr>,
}

impl CheckedUdpSocket {
    pub fn new(socket: std::net::UdpSocket, allowed_sender: Vec<SocketAddr>) -> Self {
        Self { socket, allowed_sender }
    }
}

impl NetworkSource for CheckedUdpSocket {
    type Error = CheckUdpError;

    fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error> {
        match self.socket.recv_from(buffer) {
            Ok((package_size, sender)) if self.allowed_sender.contains(&sender) => Ok(Some(package_size)),
            Ok((_, sender)) => Err(CheckUdpError::InvalidSender(sender)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Debug)]
pub enum CheckUdpError {
    IOError(std::io::Error),
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
