use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use ntest::timeout;
use rtlola_interpreter::time::AbsoluteFloat;
use rtlola_io_plugins::inputs::byte_plugin::ByteEventSource;
use rtlola_io_plugins::inputs::EventSource;

use crate::byte_plugin::socket_addr;
use crate::byte_plugin::structs::input_map::{create_events, create_monitor};
use crate::byte_plugin::structs::{check_verdict, create_verdicts};

#[test]
#[timeout(10000)]
fn tcp_listener() {
    let (receiver_addr, _sender_addr) = socket_addr();
    let thread_1 = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        let mut sender = TcpStream::connect(receiver_addr).unwrap();
        for data in create_events() {
            let buf = bincode::serialize(&data).unwrap();
            let num_bytes_send = sender.write(&buf).unwrap();
            assert_eq!(num_bytes_send, buf.len());
            thread::sleep(Duration::from_secs(1));
        }
    });
    let thread_2 = thread::spawn(move || {
        let (receiver, _connected) = TcpListener::bind(receiver_addr).unwrap().accept().unwrap();
        receiver.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        receiver.set_nonblocking(false).unwrap();
        let mut input_source = ByteEventSource::<TcpStream, _, AbsoluteFloat, 100>::from_source(receiver.into());
        let mut monitor = create_monitor();
        let mut expected_verdicts = create_verdicts().into_iter();
        while let (Some((ev, ts)), expected) = (input_source.next_event().unwrap(), expected_verdicts.next()) {
            let v = monitor.accept_event(ev, ts).unwrap();
            check_verdict(v, expected.unwrap());
        }
    });
    thread_2.join().unwrap();
    thread_1.join().unwrap();
}
