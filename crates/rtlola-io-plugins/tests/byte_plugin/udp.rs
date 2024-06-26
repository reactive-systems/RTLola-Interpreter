use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;
use std::{thread, vec};

use byteorder::LittleEndian;
use ntest::timeout;
use rtlola_interpreter::time::AbsoluteFloat;
use rtlola_io_plugins::byte_plugin::upd::{CheckUdpError, CheckedUdpSocket, UdpWrapper};
use rtlola_io_plugins::byte_plugin::{ByteEventSource, ByteEventSourceError};
use rtlola_io_plugins::EventSource;

use crate::byte_plugin::socket_addr;
use crate::byte_plugin::structs::{check_verdict, create_verdicts};

#[test]
#[timeout(8000)]
fn udp_unchecked() {
    let (receiver_addr, sender_addr) = socket_addr();
    let data = crate::byte_plugin::structs::input_map::create_events();
    let data_len = data.len();

    let thread_1 = thread::spawn(move || {
        let sender = UdpSocket::bind(sender_addr).unwrap();

        thread::sleep(Duration::from_secs(2));
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
        let mut input_source = ByteEventSource::<UdpWrapper, LittleEndian, _, AbsoluteFloat, 50>::from_source(
            UdpSocket::bind(receiver_addr).unwrap().into(),
        );
        let mut counter = 0;
        let mut monitor = crate::byte_plugin::structs::input_map::create_monitor();
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

        thread::sleep(Duration::from_secs(2));
        for data in crate::byte_plugin::structs::input_map::create_events() {
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
            ByteEventSource::<UdpWrapper, LittleEndian, _, AbsoluteFloat, 50>::from_source(receiver.into());
        let mut monitor = crate::byte_plugin::structs::input_map::create_monitor();
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
#[timeout(10000)]
fn checked_udp() {
    let (receiver_addr, sender_addr) = socket_addr();
    let intruder_port = portpicker::pick_unused_port().expect("No ports free");
    let intruder_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), intruder_port);
    println!("{} {} {}", receiver_addr, sender_addr, intruder_addr);
    let thread_sender = thread::spawn(move || {
        let sender = UdpSocket::bind(sender_addr).unwrap();

        thread::sleep(Duration::from_secs_f64(3.0));
        for data in crate::byte_plugin::structs::input_map::create_events() {
            let buf = bincode::serialize(&data).unwrap();
            let num_bytes_send = sender.send_to(&buf, &receiver_addr).unwrap();
            assert_eq!(num_bytes_send, buf.len());
            thread::sleep(Duration::from_secs(1));
        }

        let res = sender.send_to(&[], &receiver_addr).unwrap();
        assert_eq!(res, 0);
    });
    let thread_receiver = thread::spawn(move || {
        let receiver = UdpSocket::bind(receiver_addr).unwrap();
        receiver.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
        let receiver = CheckedUdpSocket::new(receiver, vec![sender_addr]);
        let mut input_source =
            ByteEventSource::<CheckedUdpSocket, LittleEndian, _, AbsoluteFloat, 100>::from_source(receiver.into());
        let mut monitor = crate::byte_plugin::structs::input_map::create_monitor();
        let mut expected_verdicts = create_verdicts().into_iter();
        loop {
            match input_source.next_event() {
                Ok(Some((ev, ts))) => {
                    let expected = expected_verdicts.next();
                    let v = monitor.accept_event(ev, ts).unwrap();
                    check_verdict(v, expected.unwrap());
                },
                Ok(None) => break,
                Err(ByteEventSourceError::Source(CheckUdpError::InvalidSender(sender))) => {
                    eprintln!("Intruder {sender}");
                    continue;
                },
                Err(e) => panic!("{e}"),
            }
        }
    });
    let thread_intruder = thread::spawn(move || {
        let intruder = UdpSocket::bind(intruder_addr).unwrap();

        thread::sleep(Duration::from_secs_f64(0.5));
        for data in crate::byte_plugin::structs::input_map::create_events() {
            let buf = bincode::serialize(&data).unwrap();
            let num_bytes_send = intruder.send_to(&buf, &receiver_addr).unwrap();
            assert_eq!(num_bytes_send, buf.len());
            thread::sleep(Duration::from_secs(1));
        }
    });
    thread_sender.join().unwrap();
    thread_receiver.join().unwrap();
    thread_intruder.join().unwrap();
}

#[test]
#[timeout(8000)]
fn udp_unchecked_with_input_macros() {
    let (receiver_addr, sender_addr) = socket_addr();
    let data = crate::byte_plugin::structs::input_macros::create_events();
    let data_len = data.len();

    let thread_1 = thread::spawn(move || {
        let sender = UdpSocket::bind(sender_addr).unwrap();

        thread::sleep(Duration::from_secs(2));
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
        let mut input_source = ByteEventSource::<UdpWrapper, LittleEndian, _, AbsoluteFloat, 50>::from_source(
            UdpSocket::bind(receiver_addr).unwrap().into(),
        );
        let mut counter = 0;
        let mut monitor = crate::byte_plugin::structs::input_macros::create_monitor();
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
