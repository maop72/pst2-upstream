// frame.rs  v1.2
// Octubre 2026

use crate::arp::ETH_TYPE_ARP;
use crate::eth::{
    write_eth_header, EthView, ETH_HEADER_SIZE, ETH_TYPE_IPV4, MAX_FRAME_SIZE,
};
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

    pub fn eth(&self) -> EthView<'_> {
        EthView {
            data: self.as_slice(),
        }
    }

    pub fn show(&self) -> String {
        self.eth().show()
    }
}

impl<'a> EthView<'a> {
    pub fn show(&self) -> String {
        let mut text = String::new();

        text.push_str(&format!(
            "Frame\n  Longitud: {} bytes\n",
            self.data.len()
        ));

        if self.data.len() < ETH_HEADER_SIZE {
            text.push_str("  Trama Ethernet incompleta");
            return text;
        }

        text.push_str("  Ethernet\n");
        text.push_str(&format!(
            "    Origen:  {}\n",
            format_mac(self.src_mac())
        ));
        text.push_str(&format!(
            "    Destino: {}\n",
            format_mac(self.dst_mac())
        ));

        let ethertype = self.ether_type();

        if ethertype == ETH_TYPE_ARP {
            text.push_str("    Tipo:    ARP\n");
            text.push_str(&self.show_arp());
            return text;
        }

        if ethertype == ETH_TYPE_IPV4 {
            text.push_str("    Tipo:    IPv4\n");
            text.push_str(&self.show_ipv4());
            return text;
        }

        text.push_str(&format!(
            "    Tipo:    desconocido (0x{:02x}{:02x})",
            ethertype[0], ethertype[1]
        ));

        text
    }

    fn show_arp(&self) -> String {
        let data = self.payload();
        let mut text = String::new();

        if data.len() < 28 {
            text.push_str("    ARP incompleto");
            return text;
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

        text.push_str("    ARP\n");
        text.push_str(&format!(
            "      Operación: {} ({})\n",
            operation, operation_name
        ));
        text.push_str(&format!(
            "      Emisor:    {} / {}\n",
            format_ip(sender_ip),
            format_mac(&sender_mac)
        ));
        text.push_str(&format!(
            "      Destino:   {} / {}",
            format_ip(target_ip),
            format_mac(&target_mac)
        ));

        text
    }

    fn show_ipv4(&self) -> String {
        let data = self.payload();
        let mut text = String::new();

        if data.len() < IP_HEADER_MIN_LEN {
            text.push_str("    IPv4 incompleto");
            return text;
        }

        let ip = Ipv4Header { data };

        text.push_str("    IPv4\n");
        text.push_str(&format!(
            "      Origen:    {}\n",
            format_ip(ip.src_ip())
        ));
        text.push_str(&format!(
            "      Destino:   {}\n",
            format_ip(ip.dst_ip())
        ));
        text.push_str(&format!(
            "      TTL:       {}\n",
            ip.ttl()
        ));

        match ip.protocol() {
            1 => {
                text.push_str("      Protocolo: ICMP\n");
                text.push_str(&self.show_icmp(&ip));
            }
            protocol => {
                text.push_str(&format!(
                    "      Protocolo: {}",
                    protocol
                ));
            }
        }

        text
    }

    fn show_icmp(&self, ip: &Ipv4Header<'_>) -> String {
        let data = ip.payload();
        let mut text = String::new();

        if data.len() < 2 {
            text.push_str("      ICMP incompleto");
            return text;
        }

        let icmp_type = data[0];
        let code = data[1];

        text.push_str("      ICMP\n");

        match icmp_type {
            0 => {
                text.push_str("        Tipo: Echo Reply");
            }

            3 => {
                text.push_str("        Tipo: Destination Unreachable\n");

                let description = match code {
                    0 => "Network Unreachable",
                    1 => "Host Unreachable",
                    _ => "código desconocido",
                };

                text.push_str(&format!(
                    "        Código: {} ({})",
                    code, description
                ));
            }

            8 => {
                text.push_str("        Tipo: Echo Request");
            }

            11 => {
                text.push_str("        Tipo: Time Exceeded\n");

                let description = match code {
                    0 => "TTL exceeded in transit",
                    _ => "código desconocido",
                };

                text.push_str(&format!(
                    "        Código: {} ({})",
                    code, description
                ));
            }

            _ => {
                text.push_str(&format!(
                    "        Tipo: {}\n",
                    icmp_type
                ));
                text.push_str(&format!(
                    "        Código: {}",
                    code
                ));
            }
        }

        text
    }
}
