// icmp — Vista de lectura y funciones de escritura para cabeceras ICMP.
//
// Layout de la cabecera ICMP:
//
//   [0]    Type
//   [1]    Code
//   [2..4] Checksum
//   [4..]  Datos (Identifier + Sequence + Payload en Echo;
//                 unused + IP original + 8 bytes en errores)
//
// Tipos usados:
//   0  — Echo Reply
//   3  — Destination Unreachable  (código 0=net, 1=host)
//   8  — Echo Request
//   11 — Time Exceeded            (código 0=TTL exceeded in transit)

use crate::eth::{write_eth_header, ETH_HEADER_SIZE, ETH_TYPE_IPV4, MAX_FRAME_SIZE};
use crate::ipv4::{write_ipv4_header, Ipv4Header, IP_HEADER_MIN_LEN, IP_PROTO_ICMP};
use crate::utils::calculate_checksum;

pub const ICMP_HEADER_MIN_LEN: usize = 8;

const ICMP_TYPE_ECHO_REPLY: u8 = 0;

// Inicio de cada campo de la cabecera ICMP
const ICMP_TYPE_OFF:       usize = 0;
const ICMP_CODE_OFF:       usize = 1;
const ICMP_CHECKSUM_START: usize = 2;
const ICMP_DATA_START:     usize = 4;

const ICMP_TYPE_DEST_UNREACHABLE: u8 = 3;
const ICMP_CODE_NET_UNREACHABLE: u8 = 0;
const ICMP_CODE_HOST_UNREACHABLE: u8 = 1;
const ICMP_TYPE_ECHO_REQUEST: u8 = 8;
const ICMP_TYPE_TIME_EXCEEDED: u8 = 11;
const ICMP_CODE_TTL_EXCEEDED: u8 = 0;

// Datos fijos del Echo Request (Identifier=1, Sequence=1, Payload="ping")
pub const ICMP_ECHO_DATA: &[u8] = &[0x00, 0x01, 0x00, 0x01, b'p', b'i', b'n', b'g'];
const ICMP_ECHO_DATA_LEN: usize = 8;

/// Tamaño exacto del frame ICMP Echo Request (constante en tiempo de compilación).
pub const ICMP_ECHO_FRAME_LEN: usize =
    ETH_HEADER_SIZE + IP_HEADER_MIN_LEN + ICMP_HEADER_MIN_LEN + ICMP_ECHO_DATA_LEN;

// Tamaño de los mensajes de error ICMP:
//   8B cabecera ICMP + 20B cabecera IP original + 8B primeros bytes del datagrama original
const ICMP_ERROR_PAYLOAD_LEN: usize = ICMP_HEADER_MIN_LEN + IP_HEADER_MIN_LEN + 8;

// =============================================================================
// Vista de lectura — zero-copy sobre &[u8]
// =============================================================================

/// Vista sobre una cabecera ICMP cruda.
pub struct IcmpHeader<'a> {
    pub data: &'a [u8],
}

impl<'a> IcmpHeader<'a> {
    pub fn icmp_type(&self) -> u8 {
        self.data[ICMP_TYPE_OFF]
    }
    pub fn is_echo_request(&self) -> bool {
        self.icmp_type() == ICMP_TYPE_ECHO_REQUEST
    }
    pub fn is_echo_reply(&self) -> bool {
        self.icmp_type() == ICMP_TYPE_ECHO_REPLY
    }
    /// Identifier + Sequence Number + Payload (todo después del checksum)
    pub fn echo_data(&self) -> &[u8] {
        &self.data[ICMP_DATA_START..]
    }
}

// =============================================================================
// Handlers de procesamiento
// =============================================================================

/// Procesa un ICMP Echo Request dirigido a `our_ip`.
/// Devuelve el frame de respuesta completo (Eth + IP + ICMP Echo Reply)
/// como array de tamaño fijo más la longitud real utilizada.
pub fn handle_echo_request(
    packet: &[u8],
    our_mac: &[u8],
    our_ip: [u8; 4],
) -> Option<([u8; MAX_FRAME_SIZE], usize)> {
    if packet.len() < ETH_HEADER_SIZE + IP_HEADER_MIN_LEN + ICMP_HEADER_MIN_LEN {
        return None;
    }
    if packet.len() > MAX_FRAME_SIZE {
        return None;
    }
    let eth = crate::eth::EthView { data: packet };
    if !eth.is_ipv4() {
        return None;
    }
    let ip = Ipv4Header { data: eth.payload() };
    if !ip.is_icmp() || ip.dst_ip() != our_ip {
        return None;
    }
    let icmp = IcmpHeader { data: ip.payload() };
    if !icmp.is_echo_request() {
        return None;
    }

    let ip_start = ETH_HEADER_SIZE;
    let icmp_start = ip_start + ip.ihl_bytes();
    let reply_len = packet.len();
    let mut res = [0u8; MAX_FRAME_SIZE];

    write_eth_header(&mut res[..ETH_HEADER_SIZE], eth.src_mac(), our_mac, ETH_TYPE_IPV4);
    write_ipv4_header(
        &mut res[ip_start..ip_start + IP_HEADER_MIN_LEN],
        ip.dst_ip(),
        ip.src_ip(),
        ip.total_length(),
        IP_PROTO_ICMP,
        64,
    );
    let echo_data = icmp.echo_data();
    write_icmp_echo_reply(&mut res[icmp_start..], echo_data);
    Some((res, reply_len))
}

/// Procesa un ICMP Echo Reply. Devuelve `true` si proviene de `expected_ip`.
pub fn handle_echo_reply(packet: &[u8], expected_ip: [u8; 4]) -> bool {
    if packet.len() < ETH_HEADER_SIZE + IP_HEADER_MIN_LEN + ICMP_HEADER_MIN_LEN {
        return false;
    }
    let eth = crate::eth::EthView { data: packet };
    if !eth.is_ipv4() {
        return false;
    }
    let ip = Ipv4Header { data: eth.payload() };
    if !ip.is_icmp() || ip.src_ip() != expected_ip {
        return false;
    }
    let icmp = IcmpHeader { data: ip.payload() };
    icmp.is_echo_reply()
}

// =============================================================================
// Funciones de escritura — Echo
// =============================================================================

/// Escribe un ICMP Echo Reply en `buf` y calcula su checksum.
pub fn write_icmp_echo_reply(buf: &mut [u8], echo_data: &[u8]) {
    buf[ICMP_TYPE_OFF] = ICMP_TYPE_ECHO_REPLY;
    buf[ICMP_CODE_OFF] = 0;
    buf[ICMP_CHECKSUM_START] = 0;
    buf[ICMP_CHECKSUM_START + 1] = 0;
    buf[ICMP_DATA_START..ICMP_DATA_START + echo_data.len()].copy_from_slice(echo_data);
    let checksum = calculate_checksum(&buf[..ICMP_DATA_START + echo_data.len()]);
    buf[ICMP_CHECKSUM_START..ICMP_DATA_START].copy_from_slice(&checksum.to_be_bytes());
}

/// Construye un frame Ethernet + IPv4 + ICMP Echo Request completo.
/// Devuelve un array de tamaño fijo ICMP_ECHO_FRAME_LEN.
pub fn write_icmp_echo_request_frame(
    our_mac: &[u8],
    our_ip: [u8; 4],
    dst_mac: &[u8],
    dst_ip: [u8; 4],
) -> [u8; ICMP_ECHO_FRAME_LEN] {
    let ip_total_len =
        (IP_HEADER_MIN_LEN + ICMP_HEADER_MIN_LEN + ICMP_ECHO_DATA_LEN) as u16;
    let mut frame = [0u8; ICMP_ECHO_FRAME_LEN];
    let ip_start = ETH_HEADER_SIZE;
    let icmp_start = ip_start + IP_HEADER_MIN_LEN;

    write_eth_header(&mut frame[..ETH_HEADER_SIZE], dst_mac, our_mac, ETH_TYPE_IPV4);
    write_ipv4_header(
        &mut frame[ip_start..ip_start + IP_HEADER_MIN_LEN],
        our_ip,
        dst_ip,
        ip_total_len.to_be_bytes(),
        IP_PROTO_ICMP,
        64,
    );
    write_icmp_echo_request(&mut frame[icmp_start..], ICMP_ECHO_DATA);
    frame
}

fn write_icmp_echo_request(buf: &mut [u8], echo_data: &[u8]) {
    buf[ICMP_TYPE_OFF] = ICMP_TYPE_ECHO_REQUEST;
    buf[ICMP_CODE_OFF] = 0;
    buf[ICMP_CHECKSUM_START] = 0;
    buf[ICMP_CHECKSUM_START + 1] = 0;
    buf[ICMP_DATA_START..ICMP_DATA_START + echo_data.len()].copy_from_slice(echo_data);
    let checksum = calculate_checksum(&buf[..ICMP_DATA_START + echo_data.len()]);
    buf[ICMP_CHECKSUM_START..ICMP_DATA_START].copy_from_slice(&checksum.to_be_bytes());
}

// =============================================================================
// Funciones de escritura — Mensajes de error
// =============================================================================

/// Construye un frame ICMP Time Exceeded (tipo 11, código 0).
///
/// `orig_ip_header` — cabecera IP del datagrama que causó el error (20 bytes).
/// `orig_8_bytes`   — primeros 8 bytes del payload de ese datagrama.
pub fn write_icmp_time_exceeded_frame(
    our_mac: &[u8],
    our_ip: [u8; 4],
    dst_mac: &[u8],
    dst_ip: [u8; 4],
    orig_ip_header: &[u8],
    orig_8_bytes: &[u8],
) -> [u8; ETH_HEADER_SIZE + IP_HEADER_MIN_LEN + ICMP_ERROR_PAYLOAD_LEN] {
    write_icmp_error_frame(
        our_mac, our_ip, dst_mac, dst_ip,
        ICMP_TYPE_TIME_EXCEEDED, ICMP_CODE_TTL_EXCEEDED,
        orig_ip_header, orig_8_bytes,
    )
}

/// Construye un frame ICMP Network Unreachable (tipo 3, código 0).
pub fn write_icmp_net_unreachable_frame(
    our_mac: &[u8],
    our_ip: [u8; 4],
    dst_mac: &[u8],
    dst_ip: [u8; 4],
    orig_ip_header: &[u8],
    orig_8_bytes: &[u8],
) -> [u8; ETH_HEADER_SIZE + IP_HEADER_MIN_LEN + ICMP_ERROR_PAYLOAD_LEN] {
    write_icmp_error_frame(
        our_mac, our_ip, dst_mac, dst_ip,
        ICMP_TYPE_DEST_UNREACHABLE, ICMP_CODE_NET_UNREACHABLE,
        orig_ip_header, orig_8_bytes,
    )
}

/// Construye un frame ICMP Host Unreachable (tipo 3, código 1).
pub fn write_icmp_host_unreachable_frame(
    our_mac: &[u8],
    our_ip: [u8; 4],
    dst_mac: &[u8],
    dst_ip: [u8; 4],
    orig_ip_header: &[u8],
    orig_8_bytes: &[u8],
) -> [u8; ETH_HEADER_SIZE + IP_HEADER_MIN_LEN + ICMP_ERROR_PAYLOAD_LEN] {
    write_icmp_error_frame(
        our_mac, our_ip, dst_mac, dst_ip,
        ICMP_TYPE_DEST_UNREACHABLE, ICMP_CODE_HOST_UNREACHABLE,
        orig_ip_header, orig_8_bytes,
    )
}

/// Función interna que construye cualquier mensaje de error ICMP.
///
/// Layout del payload ICMP de error (36 bytes):
///   [0]     Type
///   [1]     Code
///   [2..4]  Checksum
///   [4..8]  Unused (ceros)
///   [8..28] Cabecera IP original (20 bytes)
///   [28..36] Primeros 8 bytes del datagrama original
fn write_icmp_error_frame(
    our_mac: &[u8],
    our_ip: [u8; 4],
    dst_mac: &[u8],
    dst_ip: [u8; 4],
    icmp_type: u8,
    icmp_code: u8,
    orig_ip_header: &[u8],
    orig_8_bytes: &[u8],
) -> [u8; ETH_HEADER_SIZE + IP_HEADER_MIN_LEN + ICMP_ERROR_PAYLOAD_LEN] {
    const FRAME_LEN: usize = ETH_HEADER_SIZE + IP_HEADER_MIN_LEN + ICMP_ERROR_PAYLOAD_LEN;
    let mut frame = [0u8; FRAME_LEN];

    let ip_total_len = (IP_HEADER_MIN_LEN + ICMP_ERROR_PAYLOAD_LEN) as u16;
    let ip_start = ETH_HEADER_SIZE;
    let icmp_start = ip_start + IP_HEADER_MIN_LEN;

    write_eth_header(&mut frame[..ETH_HEADER_SIZE], dst_mac, our_mac, ETH_TYPE_IPV4);
    write_ipv4_header(
        &mut frame[ip_start..ip_start + IP_HEADER_MIN_LEN],
        our_ip,
        dst_ip,
        ip_total_len.to_be_bytes(),
        IP_PROTO_ICMP,
        64,
    );

    // Cabecera ICMP de error
    let icmp = &mut frame[icmp_start..];
    icmp[ICMP_TYPE_OFF] = icmp_type;
    icmp[ICMP_CODE_OFF] = icmp_code;
    // [2..4] checksum — se calcula al final
    // [4..8] unused = 0
    let hdr_len = orig_ip_header.len().min(IP_HEADER_MIN_LEN);
    icmp[8..8 + hdr_len].copy_from_slice(&orig_ip_header[..hdr_len]);
    let data_len = orig_8_bytes.len().min(8);
    icmp[28..28 + data_len].copy_from_slice(&orig_8_bytes[..data_len]);

    let checksum = calculate_checksum(&frame[icmp_start..icmp_start + ICMP_ERROR_PAYLOAD_LEN]);
    frame[icmp_start + 2..icmp_start + 4].copy_from_slice(&checksum.to_be_bytes());

    frame
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipv4::write_ipv4_header;
    use crate::utils::calculate_checksum;

    const OUR_MAC: [u8; 6] = [0xAA, 0x00, 0x00, 0x00, 0x00, 0x01];
    const OUR_IP: [u8; 4] = [10, 0, 1, 1];
    const DST_MAC: [u8; 6] = [0xBB, 0x00, 0x00, 0x00, 0x00, 0x02];
    const DST_IP: [u8; 4] = [10, 0, 1, 2];

    fn make_orig_ip_header() -> [u8; 20] {
        let mut buf = [0u8; 20];
        write_ipv4_header(&mut buf, DST_IP, OUR_IP, [0, 40], IP_PROTO_ICMP, 1);
        buf
    }

    #[test]
    fn time_exceeded_tipo_y_codigo() {
        let orig = make_orig_ip_header();
        let frame = write_icmp_time_exceeded_frame(
            &OUR_MAC, OUR_IP, &DST_MAC, DST_IP, &orig, &[0u8; 8],
        );
        let icmp_start = ETH_HEADER_SIZE + IP_HEADER_MIN_LEN;
        assert_eq!(frame[icmp_start], ICMP_TYPE_TIME_EXCEEDED);
        assert_eq!(frame[icmp_start + 1], ICMP_CODE_TTL_EXCEEDED);
    }

    #[test]
    fn net_unreachable_tipo_y_codigo() {
        let orig = make_orig_ip_header();
        let frame = write_icmp_net_unreachable_frame(
            &OUR_MAC, OUR_IP, &DST_MAC, DST_IP, &orig, &[0u8; 8],
        );
        let icmp_start = ETH_HEADER_SIZE + IP_HEADER_MIN_LEN;
        assert_eq!(frame[icmp_start], ICMP_TYPE_DEST_UNREACHABLE);
        assert_eq!(frame[icmp_start + 1], ICMP_CODE_NET_UNREACHABLE);
    }

    #[test]
    fn host_unreachable_tipo_y_codigo() {
        let orig = make_orig_ip_header();
        let frame = write_icmp_host_unreachable_frame(
            &OUR_MAC, OUR_IP, &DST_MAC, DST_IP, &orig, &[0u8; 8],
        );
        let icmp_start = ETH_HEADER_SIZE + IP_HEADER_MIN_LEN;
        assert_eq!(frame[icmp_start], ICMP_TYPE_DEST_UNREACHABLE);
        assert_eq!(frame[icmp_start + 1], ICMP_CODE_HOST_UNREACHABLE);
    }

    #[test]
    fn checksum_icmp_error_valido() {
        let orig = make_orig_ip_header();
        let frame = write_icmp_time_exceeded_frame(
            &OUR_MAC, OUR_IP, &DST_MAC, DST_IP, &orig, &[0u8; 8],
        );
        let icmp_start = ETH_HEADER_SIZE + IP_HEADER_MIN_LEN;
        assert_eq!(
            calculate_checksum(&frame[icmp_start..icmp_start + ICMP_ERROR_PAYLOAD_LEN]),
            0
        );
    }

    #[test]
    fn error_contiene_ip_header_original() {
        let orig = make_orig_ip_header();
        let frame = write_icmp_time_exceeded_frame(
            &OUR_MAC, OUR_IP, &DST_MAC, DST_IP, &orig, &[0xAB; 8],
        );
        let icmp_start = ETH_HEADER_SIZE + IP_HEADER_MIN_LEN;
        // Cabecera IP original en bytes [8..28] del payload ICMP
        assert_eq!(&frame[icmp_start + 8..icmp_start + 28], &orig);
        // Primeros 8 bytes del datagrama original en [28..36]
        assert_eq!(&frame[icmp_start + 28..icmp_start + 36], &[0xAB; 8]);
    }
}
