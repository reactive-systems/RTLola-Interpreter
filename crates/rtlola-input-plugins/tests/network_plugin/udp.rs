use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;
use std::{thread, vec};

use byteorder::LittleEndian;
use ntest::timeout;
use rtlola_input_plugins::network_plugin::upd::{CheckUdpError, CheckedUdpSocket, UdpWrapper};
use rtlola_input_plugins::network_plugin::{NetworkEventSource, NetworkEventSourceError};
use rtlola_input_plugins::EventSource;
use rtlola_interpreter::time::AbsoluteFloat;

use crate::network_plugin::structs::input_map::{create_events, create_monitor};
use crate::network_plugin::structs::{check_verdict, create_verdicts};

fn socket_addr() -> (SocketAddr, SocketAddr) {
    let receiver_port = portpicker::pick_unused_port().expect("No ports free");
    let receiver_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), receiver_port);
    let sender_port = portpicker::pick_unused_port().expect("No ports free");
    let sender_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), sender_port);
    (receiver_addr, sender_addr)
}

#[test]
#[timeout(8000)]
fn udp_unchecked() {
    let (receiver_addr, sender_addr) = socket_addr();
    let data = create_events();
    let data_len = data.len();

    let thread_1 = thread::spawn(move || {
        let sender = UdpSocket::bind(sender_addr).unwrap();

        thread::sleep(Duration::from_secs(1));
        for data in data {
            let buf = bincode::serialize(&data).unwrap();
            let num_bytes_send = sender.send_to(&buf, &receiver_addr).unwrap();
            assert_eq!(num_bytes_send, buf.len());
            thread::sleep(Duration::from_secs(1));
        }

        let res = sender.send_to(&[], &receiver_addr).unwrap();
        assert_eq!(res, 0);
    });
    let thread_2 = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        let mut input_source = NetworkEventSource::<UdpWrapper, LittleEndian, _, AbsoluteFloat, 50>::from_source(
            UdpSocket::bind(receiver_addr).unwrap().into(),
        );
        let mut counter = 0;
        let mut monitor = create_monitor();
        let mut expected_verdicts = create_verdicts().into_iter();
        while let (Some((ev, ts)), expected) = (input_source.next_event().unwrap(), expected_verdicts.next()) {
            let v = monitor.accept_event(ev, ts).unwrap();
            check_verdict(v, expected.unwrap());
            counter += 1;
            if counter == data_len {
                break;
            }
        }
    });
    thread_1.join().unwrap();
    thread_2.join().unwrap();
}

#[test]
#[timeout(8000)]
fn timed_udp() {
    let (receiver_addr, sender_addr) = socket_addr();
    let thread_1 = thread::spawn(move || {
        let sender = UdpSocket::bind(sender_addr).unwrap();

        thread::sleep(Duration::from_secs(1));
        for data in create_events() {
            let buf = bincode::serialize(&data).unwrap();
            let num_bytes_send = sender.send_to(&buf, &receiver_addr).unwrap();
            assert_eq!(num_bytes_send, buf.len());
            thread::sleep(Duration::from_secs(1));
        }

        let res = sender.send_to(&[], &receiver_addr).unwrap();
        assert_eq!(res, 0);
    });
    let thread_2 = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        let receiver = UdpSocket::bind(receiver_addr).unwrap();
        receiver.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut input_source =
            NetworkEventSource::<UdpWrapper, LittleEndian, _, AbsoluteFloat, 50>::from_source(receiver.into());
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

#[test]
#[timeout(8000)]
fn checked_udp() {
    let (receiver_addr, sender_addr) = socket_addr();
    let intruder_port = portpicker::pick_unused_port().expect("No ports free");
    let intruder_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), intruder_port);
    let thread_sender = thread::spawn(move || {
        let sender = UdpSocket::bind(sender_addr).unwrap();

        thread::sleep(Duration::from_secs(1));
        for data in create_events() {
            let buf = bincode::serialize(&data).unwrap();
            let num_bytes_send = sender.send_to(&buf, &receiver_addr).unwrap();
            assert_eq!(num_bytes_send, buf.len());
            thread::sleep(Duration::from_secs(1));
        }

        let res = sender.send_to(&[], &receiver_addr).unwrap();
        assert_eq!(res, 0);
    });
    let thread_receiver = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        let receiver = UdpSocket::bind(receiver_addr).unwrap();
        receiver.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let receiver = CheckedUdpSocket::new(receiver, vec![sender_addr]);
        let mut input_source =
            NetworkEventSource::<CheckedUdpSocket, LittleEndian, _, AbsoluteFloat, 50>::from_source(receiver.into());
        let mut monitor = create_monitor();
        let mut expected_verdicts = create_verdicts().into_iter();
        loop {
            match input_source.next_event() {
                Ok(Some((ev, ts))) => {
                    let expected = expected_verdicts.next();
                    let v = monitor.accept_event(ev, ts).unwrap();
                    check_verdict(dbg!(v), dbg!(expected.unwrap()));
                },
                Ok(None) => break,
                Err(NetworkEventSourceError::Source(CheckUdpError::InvalidSender(_))) => continue,
                Err(e) => panic!("{e}"),
            }
        }
    });
    let thread_intruder = thread::spawn(move || {
        let intruder = UdpSocket::bind(intruder_addr).unwrap();

        thread::sleep(Duration::from_secs(1));
        for data in create_events() {
            let buf = bincode::serialize(&data).unwrap();
            let num_bytes_send = intruder.send_to(&buf, &receiver_addr).unwrap();
            assert_eq!(num_bytes_send, buf.len());
            thread::sleep(Duration::from_secs(1));
        }

        let res = intruder.send_to(&[], &receiver_addr).unwrap();
        assert_eq!(res, 0);
    });
    thread_sender.join().unwrap();
    thread_receiver.join().unwrap();
    thread_intruder.join().unwrap();
}
