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
    // Direcciones ethernet de origen y destino.
    // 6 bytes cada una
    let dst = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    let src = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];

    // Protocolo o tipo de datos en el payload. 2 bytes.
    // Usaremos este valor que no se corresponde con ningún tipo real
    let ethertype = [0x12, 0x34];

    // Datos de la trama. 
    // Máximo 1500 bytes.
    // Mínimo 46 bytes. (Los niveles inferiores completan el mínimo
    // si es necesario)
    let payload = [0x01, 0x02, 0x03, 0x04, 0x05];

    // Crea la trama
    let frame = Frame::from_eth(dst, src, ethertype, &payload);

    // Crea una vista para acceder a la información Ethernet
    let eth = frame.eth();

    // La cadena de formato "{:02x?}" representa los valores en hexa.
    // Podríamos usar "{:?}" para verlo en la representación de depuración,
    // que en este caso es decimal.
    println!("Origen:    {:02x?}", eth.src_mac());   
    println!("Destino:   {:02x?}", eth.dst_mac());
    println!("Tipo:      {:02x?}", eth.ether_type());
    println!("Payload:   {:02x?}", eth.payload());

    println!("¿Es IPv4?: {}", eth.is_ipv4());
    println!("¿Es ARP?:  {}", eth.is_arp());

    // Mostrar la trama
    println!();
    println!("{}", frame.show());
}
