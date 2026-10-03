// =============================================================================
// Buffer para una trama Ethernet de tamaño fijo
// =============================================================================

use crate::arp::ETH_TYPE_ARP;
use crate::eth::{write_eth_header, EthView, ETH_HEADER_SIZE, ETH_TYPE_IPV4, MAX_FRAME_SIZE};
use crate::ipv4::{Ipv4Header, IP_HEADER_MIN_LEN};
use crate::utils::{format_ip, format_mac};

#[derive(Clone, Copy)]
pub struct Frame {
    pub data: [u8; MAX_FRAME_SIZE],
    pub len: usize,
}

impl Frame {
    pub fn new() -> Self {
        Frame {
            data: [0u8; MAX_FRAME_SIZE],
            len: 0,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }

    // =========================================================================
    // API 1.1
    // =========================================================================

    /// Crea un Frame a partir de una cabecera Ethernet y su payload.
    pub fn from_eth(
        dst: [u8; 6],
        src: [u8; 6],
        ethertype: [u8; 2],
        payload: &[u8],
    ) -> Frame {
        let len = ETH_HEADER_SIZE + payload.len();

        assert!(
            len <= MAX_FRAME_SIZE,
            "La trama supera el tamaño máximo de {} bytes",
            MAX_FRAME_SIZE
        );

        let mut frame = Frame::new();

        write_eth_header(
            &mut frame.data[..ETH_HEADER_SIZE],
            &dst,
            &src,
            ethertype,
        );

        frame.data[ETH_HEADER_SIZE..len].copy_from_slice(payload);
        frame.len = len;

        frame
    }

    /// Devuelve una vista de la cabecera Ethernet de la trama.
    pub fn eth(&self) -> EthView<'_> {
        EthView {
            data: self.as_slice(),
        }
    }

    /// Muestra información interpretada sobre la trama.
    pub fn show(&self) {
        println!("Frame");
        println!("  Longitud: {} bytes", self.len);

        if self.len < ETH_HEADER_SIZE {
            println!("  Trama Ethernet incompleta");
            return;
        }

        let eth = self.eth();

        println!("  Ethernet");
        println!("    Origen:  {}", format_mac(eth.src_mac()));
        println!("    Destino: {}", format_mac(eth.dst_mac()));

        let ethertype = eth.ether_type();

        if ethertype == ETH_TYPE_ARP {
            println!("    Tipo:    ARP");
            self.show_arp();
            return;
        }

        if ethertype == ETH_TYPE_IPV4 {
            println!("    Tipo:    IPv4");
            self.show_ipv4();
            return;
        }

        println!(
            "    Tipo:    desconocido (0x{:02x}{:02x})",
            ethertype[0], ethertype[1]
        );
    }

    // =========================================================================
    // Funciones privadas utilizadas por show()
    // =========================================================================

    fn show_arp(&self) {
        let eth = self.eth();
        let data = eth.payload();

        // Cabecera ARP Ethernet/IPv4:
        //
        //   0..2   Hardware type
        //   2..4   Protocol type
        //   4     Hardware address length
        //   5     Protocol address length
        //   6..8   Operation
        //   8..14  Sender hardware address
        //   14..18 Sender protocol address
        //   18..24 Target hardware address
        //   24..28 Target protocol address

        if data.len() < 28 {
            println!("    ARP incompleto");
            return;
        }

        let operation = u16::from_be_bytes([data[6], data[7]]);

        let sender_mac = [
            data[8], data[9], data[10],
            data[11], data[12], data[13],
        ];

        let sender_ip = [
            data[14], data[15], data[16], data[17],
        ];

        let target_mac = [
            data[18], data[19], data[20],
            data[21], data[22], data[23],
        ];

        let target_ip = [
            data[24], data[25], data[26], data[27],
        ];

        let operation_name = match operation {
            1 => "Request",
            2 => "Reply",
            _ => "desconocida",
        };

        println!("    ARP");
        println!(
            "      Operación: {} ({})",
            operation, operation_name
        );
        println!(
            "      Emisor:    {} / {}",
            format_ip(sender_ip),
            format_mac(&sender_mac)
        );
        println!(
            "      Destino:   {} / {}",
            format_ip(target_ip),
            format_mac(&target_mac)
        );
    }

    fn show_ipv4(&self) {
        let eth = self.eth();
        let data = eth.payload();

        if data.len() < IP_HEADER_MIN_LEN {
            println!("    IPv4 incompleto");
            return;
        }

        let ip = Ipv4Header { data };

        let protocol = ip.protocol();

        println!("    IPv4");
        println!("      Origen:    {}", format_ip(ip.src_ip()));
        println!("      Destino:   {}", format_ip(ip.dst_ip()));
        println!("      TTL:       {}", ip.ttl());

        match protocol {
            1 => {
                println!("      Protocolo: ICMP");
                self.show_icmp(&ip);
            }
            _ => {
                println!("      Protocolo: {}", protocol);
            }
        }
    }

    fn show_icmp(&self, ip: &Ipv4Header<'_>) {
        let data = ip.payload();

        if data.len() < 2 {
            println!("      ICMP incompleto");
            return;
        }

        let icmp_type = data[0];
        let code = data[1];

        match icmp_type {
            0 => {
                println!("      ICMP");
                println!("        Tipo: Echo Reply");
            }

            3 => {
                println!("      ICMP");
                println!("        Tipo: Destination Unreachable");

                let description = match code {
                    0 => "Network Unreachable",
                    1 => "Host Unreachable",
                    _ => "código desconocido",
                };

                println!("        Código: {} ({})", code, description);
            }

            8 => {
                println!("      ICMP");
                println!("        Tipo: Echo Request");
            }

            11 => {
                println!("      ICMP");
                println!("        Tipo: Time Exceeded");

                let description = match code {
                    0 => "TTL exceeded in transit",
                    _ => "código desconocido",
                };

                println!("        Código: {} ({})", code, description);
            }

            _ => {
                println!("      ICMP");
                println!("        Tipo: {}", icmp_type);
                println!("        Código: {}", code);
            }
        }
    }
}
