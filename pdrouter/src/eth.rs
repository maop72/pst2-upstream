// eth — Vista de lectura y función de escritura para frames Ethernet.
//
// Layout de la cabecera Ethernet (14 bytes):
//
//   ┌──────────┬──────────┬───────────┐
//   │ DST MAC  │ SRC MAC  │ EtherType │
//   │ 6 bytes  │ 6 bytes  │  2 bytes  │
//   └──────────┴──────────┴───────────┘

pub const ETH_HEADER_SIZE: usize = 14;
pub const ETH_TYPE_IPV4: [u8; 2] = [0x08, 0x00];
pub const MAX_FRAME_SIZE: usize = 1514; // MTU (1500) + cabecera Ethernet (14)

// =============================================================================
// Vista de lectura — zero-copy sobre &[u8]
// =============================================================================

/// Vista sobre un frame Ethernet crudo.
pub struct EthView<'a> {
    pub data: &'a [u8],
}

impl<'a> EthView<'a> {
    pub fn dst_mac(&self) -> &[u8] {
        &self.data[0..6]
    }
    pub fn src_mac(&self) -> &[u8] {
        &self.data[6..12]
    }
    pub fn ether_type(&self) -> [u8; 2] {
        [self.data[12], self.data[13]]
    }
    pub fn is_ipv4(&self) -> bool {
        self.ether_type() == ETH_TYPE_IPV4
    }
    pub fn is_arp(&self) -> bool {
        self.ether_type() == crate::arp::ETH_TYPE_ARP
    }
    pub fn payload(&self) -> &[u8] {
        &self.data[ETH_HEADER_SIZE..]
    }
}

// =============================================================================
// Función de escritura
// =============================================================================

/// Escribe la cabecera Ethernet en `buf[0..14]`.
pub fn write_eth_header(buf: &mut [u8], dst_mac: &[u8], src_mac: &[u8], ethertype: [u8; 2]) {
    buf[0..6].copy_from_slice(dst_mac);
    buf[6..12].copy_from_slice(src_mac);
    buf[12..14].copy_from_slice(&ethertype);
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arp::ETH_TYPE_ARP;

    const DST: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    const SRC: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];

    fn make_frame(ethertype: [u8; 2]) -> [u8; ETH_HEADER_SIZE] {
        let mut buf = [0u8; ETH_HEADER_SIZE];
        write_eth_header(&mut buf, &DST, &SRC, ethertype);
        buf
    }

    #[test]
    fn write_y_dst_mac() {
        let buf = make_frame(ETH_TYPE_IPV4);
        let frame = EthView { data: &buf };
        assert_eq!(frame.dst_mac(), &DST);
    }

    #[test]
    fn write_y_src_mac() {
        let buf = make_frame(ETH_TYPE_IPV4);
        let frame = EthView { data: &buf };
        assert_eq!(frame.src_mac(), &SRC);
    }

    #[test]
    fn write_y_ethertype_ipv4() {
        let buf = make_frame(ETH_TYPE_IPV4);
        let frame = EthView { data: &buf };
        assert_eq!(frame.ether_type(), ETH_TYPE_IPV4);
        assert!(frame.is_ipv4());
        assert!(!frame.is_arp());
    }

    #[test]
    fn write_y_ethertype_arp() {
        let buf = make_frame(ETH_TYPE_ARP);
        let frame = EthView { data: &buf };
        assert_eq!(frame.ether_type(), ETH_TYPE_ARP);
        assert!(frame.is_arp());
        assert!(!frame.is_ipv4());
    }

    #[test]
    fn payload_empieza_en_byte_14() {
        let mut data = [0u8; ETH_HEADER_SIZE + 4];
        write_eth_header(&mut data[..ETH_HEADER_SIZE], &DST, &SRC, ETH_TYPE_IPV4);
        data[ETH_HEADER_SIZE..].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let frame = EthView { data: &data };
        assert_eq!(frame.payload(), &[0xDE, 0xAD, 0xBE, 0xEF]);
    }
}
