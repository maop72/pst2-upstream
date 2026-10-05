// ipv4 — Vista de lectura y funciones de escritura/modificación para cabeceras IPv4.
//
// Layout de la cabecera IPv4 (20 bytes mínimo):
//
//   [0]      Version(4b) + IHL(4b)
//   [1]      DSCP/ECN
//   [2..4]   Total Length
//   [4..6]   Identification
//   [6..8]   Flags + Fragment Offset
//   [8]      TTL
//   [9]      Protocol
//   [10..12] Header Checksum
//   [12..16] Source IP
//   [16..20] Destination IP

use crate::utils::calculate_checksum;

pub const IP_HEADER_MIN_LEN: usize = 20;
pub const IP_PROTO_ICMP: u8 = 1;

// Inicio de cada campo de la cabecera IPv4 (20 bytes)
const IP_VER_IHL_OFF: usize = 0;
const IP_TOTAL_LEN_START: usize = 2;
const IP_TTL_OFF: usize = 8;
const IP_PROTO_OFF: usize = 9;
const IP_CHECKSUM_START: usize = 10;
const IP_SRC_START: usize = 12;
const IP_DST_START: usize = 16;

// =============================================================================
// Vista de lectura — zero-copy sobre &[u8]
// =============================================================================

/// Vista sobre una cabecera IPv4 cruda.
/// El slice `data` empieza en el byte 0 del header IP.
pub struct Ipv4Header<'a> {
    pub data: &'a [u8],
}

impl<'a> Ipv4Header<'a> {
    pub fn ihl_bytes(&self) -> usize {
        (self.data[IP_VER_IHL_OFF] & 0x0F) as usize * 4
    }
    pub fn total_length(&self) -> [u8; 2] {
        [
            self.data[IP_TOTAL_LEN_START],
            self.data[IP_TOTAL_LEN_START + 1],
        ]
    }
    pub fn ttl(&self) -> u8 {
        self.data[IP_TTL_OFF]
    }
    pub fn protocol(&self) -> u8 {
        self.data[IP_PROTO_OFF]
    }
    pub fn src_ip(&self) -> [u8; 4] {
        [
            self.data[IP_SRC_START],
            self.data[IP_SRC_START + 1],
            self.data[IP_SRC_START + 2],
            self.data[IP_SRC_START + 3],
        ]
    }
    pub fn dst_ip(&self) -> [u8; 4] {
        [
            self.data[IP_DST_START],
            self.data[IP_DST_START + 1],
            self.data[IP_DST_START + 2],
            self.data[IP_DST_START + 3],
        ]
    }
    pub fn is_icmp(&self) -> bool {
        self.protocol() == IP_PROTO_ICMP
    }
    pub fn payload(&self) -> &[u8] {
        &self.data[self.ihl_bytes()..]
    }
    /// Devuelve los primeros 8 bytes del payload IP (para mensajes ICMP de error).
    pub fn first_8_payload_bytes(&self) -> &[u8] {
        let start = self.ihl_bytes();
        let end = (start + 8).min(self.data.len());
        &self.data[start..end]
    }
}

// =============================================================================
// Funciones de escritura y modificación
// =============================================================================

/// Escribe la cabecera IPv4 en `buf[0..20]` y calcula su checksum.
pub fn write_ipv4_header(
    buf: &mut [u8],
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    total_len: [u8; 2],
    protocol: u8,
    ttl: u8,
) {
    buf[IP_VER_IHL_OFF] = 0x45; // Version 4, IHL 5 (20 bytes, sin opciones)
    buf[1] = 0;
    buf[IP_TOTAL_LEN_START..IP_TOTAL_LEN_START + 2].copy_from_slice(&total_len);
    buf[4..6].copy_from_slice(&[0, 0]); // Identification
    buf[6..8].copy_from_slice(&[0, 0]); // Flags + Fragment Offset
    buf[IP_TTL_OFF] = ttl;
    buf[IP_PROTO_OFF] = protocol;
    buf[IP_CHECKSUM_START..IP_CHECKSUM_START + 2].copy_from_slice(&[0, 0]); // Checksum en 0 para el cálculo
    buf[IP_SRC_START..IP_DST_START].copy_from_slice(&src_ip);
    buf[IP_DST_START..IP_DST_START + 4].copy_from_slice(&dst_ip);

    let checksum = calculate_checksum(&buf[..IP_HEADER_MIN_LEN]);
    buf[IP_CHECKSUM_START..IP_CHECKSUM_START + 2].copy_from_slice(&checksum.to_be_bytes());
}

/// Decrementa el TTL y recalcula el checksum del header IP en el buffer.
/// El buffer debe empezar en el byte 0 del header IP y tener al menos 20 bytes.
/// Devuelve el nuevo TTL (0 si ya era 0 o 1).
pub fn decrement_ttl_and_recompute_checksum(ip_buf: &mut [u8]) -> u8 {
    if ip_buf[IP_TTL_OFF] == 0 {
        return 0;
    }
    ip_buf[IP_TTL_OFF] -= 1;
    // Poner checksum a 0 y recalcular
    ip_buf[IP_CHECKSUM_START] = 0;
    ip_buf[IP_CHECKSUM_START + 1] = 0;
    let checksum = calculate_checksum(&ip_buf[..IP_HEADER_MIN_LEN]);
    ip_buf[IP_CHECKSUM_START..IP_CHECKSUM_START + 2].copy_from_slice(&checksum.to_be_bytes());
    ip_buf[IP_TTL_OFF]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ipv4_header(src: [u8; 4], dst: [u8; 4], ttl: u8, protocol: u8) -> [u8; 20] {
        let mut buf = [0u8; 20];
        write_ipv4_header(&mut buf, src, dst, [0, 20], protocol, ttl);
        buf
    }

    #[test]
    fn write_and_parse_roundtrip() {
        let buf = make_ipv4_header([10, 0, 1, 1], [10, 0, 2, 1], 64, IP_PROTO_ICMP);
        let hdr = Ipv4Header { data: &buf };
        assert_eq!(hdr.src_ip(), [10, 0, 1, 1]);
        assert_eq!(hdr.dst_ip(), [10, 0, 2, 1]);
        assert_eq!(hdr.ttl(), 64);
        assert_eq!(hdr.protocol(), IP_PROTO_ICMP);
        assert!(hdr.is_icmp());
    }

    #[test]
    fn checksum_valido_tras_write() {
        let buf = make_ipv4_header([10, 0, 1, 1], [10, 0, 2, 1], 64, IP_PROTO_ICMP);
        // Si el checksum es correcto, recalcularlo sobre los 20 bytes da 0
        assert_eq!(calculate_checksum(&buf), 0);
    }

    #[test]
    fn decrement_ttl_reduce_en_uno() {
        let mut buf = make_ipv4_header([10, 0, 1, 1], [10, 0, 2, 1], 10, IP_PROTO_ICMP);
        let nuevo = decrement_ttl_and_recompute_checksum(&mut buf);
        assert_eq!(nuevo, 9);
        assert_eq!(Ipv4Header { data: &buf }.ttl(), 9);
    }

    #[test]
    fn checksum_valido_tras_decrement() {
        let mut buf = make_ipv4_header([10, 0, 1, 1], [10, 0, 2, 1], 64, IP_PROTO_ICMP);
        decrement_ttl_and_recompute_checksum(&mut buf);
        assert_eq!(calculate_checksum(&buf), 0);
    }

    #[test]
    fn decrement_ttl_desde_1_da_0() {
        let mut buf = make_ipv4_header([10, 0, 1, 1], [10, 0, 2, 1], 1, IP_PROTO_ICMP);
        let nuevo = decrement_ttl_and_recompute_checksum(&mut buf);
        assert_eq!(nuevo, 0);
    }

    #[test]
    fn decrement_ttl_desde_0_no_cambia() {
        let mut buf = make_ipv4_header([10, 0, 1, 1], [10, 0, 2, 1], 0, IP_PROTO_ICMP);
        let nuevo = decrement_ttl_and_recompute_checksum(&mut buf);
        assert_eq!(nuevo, 0);
        assert_eq!(Ipv4Header { data: &buf }.ttl(), 0);
    }
}
