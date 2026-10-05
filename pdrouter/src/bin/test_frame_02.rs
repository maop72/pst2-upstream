// test_frame_02.rs
// PDRouter. Test del API v1.2
// Ejecutar con
// cargo run --bin test_frame_02



#[path = "../arp.rs"]
mod arp;

#[path = "../eth.rs"]
mod eth;

#[path = "../ipv4.rs"]
mod ipv4;

#[path = "../utils.rs"]
mod utils;

#[path = "../frame.rs"]
mod frame;

use frame::Frame;

fn main() {
    let dst = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    let src = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];

    // ------------------------------------------------------------
    // ARP Request
    // ------------------------------------------------------------

    let arp_request = [
        0x00, 0x01,             // Hardware type: Ethernet
        0x08, 0x00,             // Protocol type: IPv4
        0x06,                   // Hardware address length
        0x04,                   // Protocol address length
        0x00, 0x01,             // Operation: Request

        // Sender MAC
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55,

        // Sender IP: 192.168.1.10
        192, 168, 1, 10,

        // Target MAC
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00,

        // Target IP: 192.168.1.1
        192, 168, 1, 1,
    ];

    let frame = Frame::from_eth(
        dst,
        src,
        [0x08, 0x06],
        &arp_request,
    );

    println!("================ ARP Request ================");
    println!("{}", frame.show());

    // ------------------------------------------------------------
    // IPv4 + ICMP Echo Request
    // ------------------------------------------------------------

    let icmp_echo_request = [
        8,                      // Type: Echo Request
        0,                      // Code
        0, 0,                   // Checksum (no se comprueba en este test)
        0x12, 0x34,             // Identifier
        0x00, 0x01,             // Sequence number
    ];

    let frame = ipv4_icmp_frame(
        dst,
        src,
        [192, 168, 1, 10],
        [192, 168, 1, 1],
        64,
        &icmp_echo_request,
    );

    println!();
    println!("================ ICMP Echo Request ================");
    println!("{}", frame.show());

    // ------------------------------------------------------------
    // IPv4 + ICMP Echo Reply
    // ------------------------------------------------------------

    let icmp_echo_reply = [
        0,                      // Type: Echo Reply
        0,                      // Code
        0, 0,                   // Checksum
        0x12, 0x34,             // Identifier
        0x00, 0x01,             // Sequence number
    ];

    let frame = ipv4_icmp_frame(
        dst,
        src,
        [192, 168, 1, 1],
        [192, 168, 1, 10],
        64,
        &icmp_echo_reply,
    );

    println!();
    println!("================ ICMP Echo Reply ================");
    println!("{}", frame.show());

    // ------------------------------------------------------------
    // IPv4 + ICMP Destination Unreachable
    // Network Unreachable
    // ------------------------------------------------------------

    let icmp_network_unreachable = [
        3,                      // Type: Destination Unreachable
        0,                      // Code: Network Unreachable
        0, 0,                   // Checksum
        0, 0, 0, 0,             // Unused
    ];

    let frame = ipv4_icmp_frame(
        dst,
        src,
        [192, 168, 1, 1],
        [192, 168, 1, 10],
        64,
        &icmp_network_unreachable,
    );

    println!();
    println!("================ ICMP Network Unreachable ================");
    println!("{}", frame.show());

    // ------------------------------------------------------------
    // IPv4 + ICMP Destination Unreachable
    // Host Unreachable
    // ------------------------------------------------------------

    let icmp_host_unreachable = [
        3,                      // Type: Destination Unreachable
        1,                      // Code: Host Unreachable
        0, 0,                   // Checksum
        0, 0, 0, 0,             // Unused
    ];

    let frame = ipv4_icmp_frame(
        dst,
        src,
        [192, 168, 1, 1],
        [192, 168, 1, 10],
        64,
        &icmp_host_unreachable,
    );

    println!();
    println!("================ ICMP Host Unreachable ================");
    println!("{}", frame.show());

    // ------------------------------------------------------------
    // IPv4 + ICMP Time Exceeded
    // ------------------------------------------------------------

    let icmp_time_exceeded = [
        11,                     // Type: Time Exceeded
        0,                      // Code: TTL exceeded in transit
        0, 0,                   // Checksum
        0, 0, 0, 0,             // Unused
    ];

    let frame = ipv4_icmp_frame(
        dst,
        src,
        [192, 168, 1, 1],
        [192, 168, 1, 10],
        64,
        &icmp_time_exceeded,
    );

    println!();
    println!("================ ICMP Time Exceeded ================");
    println!("{}", frame.show());
}


// Crea una trama Ethernet que contiene un paquete IPv4
// cuyo payload es un mensaje ICMP.
fn ipv4_icmp_frame(
    dst: [u8; 6],
    src: [u8; 6],
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    ttl: u8,
    icmp: &[u8],
) -> Frame {
    let total_length = 20 + icmp.len();

    let mut ipv4 = Vec::with_capacity(total_length);

    // Versión 4, IHL = 5 (20 bytes)
    ipv4.push(0x45);

    // DSCP/ECN
    ipv4.push(0);

    // Longitud total
    ipv4.extend_from_slice(&(total_length as u16).to_be_bytes());

    // Identification
    ipv4.extend_from_slice(&[0, 0]);

    // Flags + fragment offset
    ipv4.extend_from_slice(&[0x40, 0x00]);

    // TTL
    ipv4.push(ttl);

    // Protocol: ICMP
    ipv4.push(1);

    // Checksum
    ipv4.extend_from_slice(&[0, 0]);

    // IP origen
    ipv4.extend_from_slice(&src_ip);

    // IP destino
    ipv4.extend_from_slice(&dst_ip);

    // ICMP
    ipv4.extend_from_slice(icmp);

    Frame::from_eth(
        dst,
        src,
        [0x08, 0x00],
        &ipv4,
    )
}
