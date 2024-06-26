use std::io::Write;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

use byteorder::LittleEndian;
use net2::TcpBuilder;
use ntest::timeout;
use rtlola_interpreter::time::AbsoluteFloat;
use rtlola_io_plugins::network_plugin::NetworkEventSource;
use rtlola_io_plugins::EventSource;

use crate::network_plugin::socket_addr;
use crate::network_plugin::structs::input_map::{create_events, create_monitor};
use crate::network_plugin::structs::{check_verdict, create_verdicts};

#[test]
#[timeout(6000)]
#[ignore = "Connection Refused"]
fn tcp_listener() {
    let (receiver_addr, sender_addr) = socket_addr();
    println!("{receiver_addr}, {sender_addr}");
    let thread_1 = thread::spawn(move || {
        let binding = TcpBuilder::new_v4().unwrap();
        let sender = binding.bind(sender_addr).unwrap();
        thread::sleep(Duration::from_secs(1));
        let mut sender = loop {
            match sender.connect(receiver_addr) {
                Ok(sender) => break sender,
                Err(e) => {
                    thread::sleep(Duration::from_secs_f32(1.0));
                    eprintln!("Here {e}")
                },
            }
        };
        for data in create_events() {
            let buf = bincode::serialize(&data).unwrap();
            let num_bytes_send = sender.write(&buf).unwrap();
            assert_eq!(num_bytes_send, buf.len());
            thread::sleep(Duration::from_secs(1));
        }
    });
    let thread_2 = thread::spawn(move || {
        let (receiver, connected) = TcpBuilder::new_v4()
            .unwrap()
            .bind(receiver_addr)
            .unwrap()
            .to_tcp_listener()
            .unwrap()
            .accept()
            .unwrap();
        assert_eq!(connected, sender_addr);
        let mut input_source =
            NetworkEventSource::<TcpStream, LittleEndian, _, AbsoluteFloat, 50>::from_source(receiver.into());
        let mut monitor = create_monitor();
        let mut expected_verdicts = create_verdicts().into_iter();
        while let (Some((ev, ts)), expected) = (input_source.next_event().unwrap(), expected_verdicts.next()) {
            let v = monitor.accept_event(ev, ts).unwrap();
            check_verdict(v, expected.unwrap());
        }
    });
    thread_1.join().unwrap();
    thread_2.join().unwrap();
}
