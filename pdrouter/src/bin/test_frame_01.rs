// test_frame_01.rs
// PDRouter. Test del API v1.2
// Ejecutar con
// cargo run --bin test_frame_01

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
    let ethertype = [0x12, 0x34];

    let payload = [1, 2, 3, 4, 5, 6, 7];

    // Crear la trama
    let frame = Frame::from_eth(
        dst,
        src,
        ethertype,
        &payload,
    );

    // Acceder a la cabecera Ethernet
    let eth = frame.eth();

    println!("Origen:    {:?}", eth.src_mac());
    println!("Destino:   {:?}", eth.dst_mac());
    println!("Tipo:      {:02x?}", eth.ether_type());
    println!("Payload:   {:?}", eth.payload());

    println!("¿Es IPv4?: {}", eth.is_ipv4());
    println!("¿Es ARP?:  {}", eth.is_arp());

    // Mostrar la trama
    println!();
    println!("{}", frame.show());
}
