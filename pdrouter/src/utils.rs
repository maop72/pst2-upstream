// utils — Checksum RFC 1071, helpers de formato y parseo.

// =============================================================================
// Checksum RFC 1071 — complemento a 1
// =============================================================================

pub fn calculate_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < data.len() - 1 {
        let word = ((data[i] as u32) << 8) | (data[i + 1] as u32);
        sum += word;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

// =============================================================================
// Helpers de formato
// =============================================================================

pub fn format_ip(ip: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
}

pub fn format_mac(mac: &[u8]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

// =============================================================================
// Parseo
// =============================================================================

pub fn parse_ipv4(s: &str) -> Result<[u8; 4], String> {
    let mut bytes = [0u8; 4];
    let mut rest = s;
    for i in 0..4 {
        let (octet_str, remainder) = if i < 3 {
            rest.split_once('.')
                .ok_or_else(|| format!("se esperaban 4 octetos en '{s}'"))?
        } else {
            (rest, "")
        };
        bytes[i] = octet_str
            .parse::<u8>()
            .map_err(|e| format!("octeto {}: {}", i + 1, e))?;
        rest = remainder;
    }
    if !rest.is_empty() {
        return Err(format!("demasiados octetos en '{s}'"));
    }
    Ok(bytes)
}

/// Parsea una máscara en notación decimal punteada o en prefijo CIDR (/24).
pub fn parse_mask(s: &str) -> Result<[u8; 4], String> {
    if let Some(bits_str) = s.strip_prefix('/') {
        let bits: u8 = bits_str
            .parse()
            .map_err(|_| format!("prefijo CIDR inválido: {s}"))?;
        if bits > 32 {
            return Err(format!("prefijo CIDR fuera de rango: {bits}"));
        }
        let mask: u32 = if bits == 0 { 0 } else { !0u32 << (32 - bits) };
        Ok(mask.to_be_bytes())
    } else {
        parse_ipv4(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_all_zeros() {
        // Complemento a 1 de cero es 0xFFFF
        let data = [0u8; 20];
        assert_eq!(calculate_checksum(&data), 0xFFFF);
    }

    #[test]
    fn format_ip_basic() {
        assert_eq!(format_ip([10, 0, 1, 1]), "10.0.1.1");
    }

    #[test]
    fn parse_ipv4_ok() {
        assert_eq!(parse_ipv4("192.168.1.1").unwrap(), [192, 168, 1, 1]);
    }

    #[test]
    fn parse_ipv4_err_octetos() {
        assert!(parse_ipv4("192.168.1").is_err());
    }

    #[test]
    fn parse_mask_cidr() {
        assert_eq!(parse_mask("/24").unwrap(), [255, 255, 255, 0]);
        assert_eq!(parse_mask("/0").unwrap(), [0, 0, 0, 0]);
        assert_eq!(parse_mask("/32").unwrap(), [255, 255, 255, 255]);
    }

    #[test]
    fn parse_mask_dotted() {
        assert_eq!(parse_mask("255.255.0.0").unwrap(), [255, 255, 0, 0]);
    }
}
