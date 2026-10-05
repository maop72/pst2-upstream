// arp — Protocolo ARP y caché ARP.
//
// Permite consultar los campos de un paquete ARP que se haya
// recibido en una trama Ethernet, y componer una trama Ethernet
// con un nuevo paquete ARP para enviarlo.
//
//
// Estructura de una trama Ethernet que contiene un paquete
// ARP (RFC 826):
//
//   0               1               2               3
//   0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |                  Destination MAC (bytes 0-3)                  |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |      Dst MAC (bytes 4-5)      |      Src MAC (bytes 0-1)      |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |                     Source MAC (bytes 2-5)                    |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |           EtherType           |         Hardware Type         |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |         Protocol Type         |     HW Len    |     Pr Len    |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |           Operation           |     Sender MAC (bytes 0-1)    |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |                     Sender MAC (bytes 2-5)                    |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |                           Sender IP                           |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |                     Target MAC (bytes 0-3)                    |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |     Target MAC (bytes 4-5)    |     Target IP (bytes 0-1)     |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |     Target IP (bytes 2-3)     |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+

use crate::eth::{write_eth_header, ETH_HEADER_SIZE};
use std::time::{Duration, Instant};

pub const ETH_TYPE_ARP: [u8; 2] = [0x08, 0x06];

const ARP_HTYPE: [u8; 2] = [0x00, 0x01];
const ARP_PTYPE: [u8; 2] = [0x08, 0x00];
const ARP_HLEN: u8 = 6;
const ARP_PLEN: u8 = 4;
const ARP_OP_REQUEST: [u8; 2] = [0x00, 0x01];
const ARP_OP_REPLY: [u8; 2] = [0x00, 0x02];
const MAC_BROADCAST: [u8; 6] = [0xFF; 6];
const MAC_ZERO: [u8; 6] = [0x00; 6];
pub const ARP_PAYLOAD_LEN: usize = 28;
pub const ARP_FRAME_SIZE: usize = ETH_HEADER_SIZE + ARP_PAYLOAD_LEN; // 42 bytes

// Tamaños de campo fijos definidos por el protocolo
const ARP_HTYPE_LEN: usize = 2;
const ARP_PTYPE_LEN: usize = 2;
const ARP_OP_LEN: usize = 2;

// Inicio de cada campo (relativo al byte 0 del payload ARP)
const ARP_HTYPE_START: usize = 0;
const ARP_PTYPE_START: usize = ARP_HTYPE_START + ARP_HTYPE_LEN;
const ARP_HLEN_OFF: usize = ARP_PTYPE_START + ARP_PTYPE_LEN;
const ARP_PLEN_OFF: usize = ARP_HLEN_OFF + 1;
const ARP_OP_START: usize = ARP_PLEN_OFF + 1;
const ARP_SENDER_MAC_START: usize = ARP_OP_START + ARP_OP_LEN;
const ARP_SENDER_IP_START: usize = ARP_SENDER_MAC_START + ARP_HLEN as usize;
const ARP_TARGET_MAC_START: usize = ARP_SENDER_IP_START + ARP_PLEN as usize;
const ARP_TARGET_IP_START: usize = ARP_TARGET_MAC_START + ARP_HLEN as usize;

// =============================================================================
// Vista de lectura — zero-copy sobre el payload ARP (28 bytes)
// =============================================================================

/// Vista sobre el payload ARP que sigue a la cabecera Ethernet (offset 14).
pub struct ArpView<'a> {
    pub data: &'a [u8],
}

impl<'a> ArpView<'a> {
    pub fn is_valid(&self) -> bool {
        self.data.len() >= ARP_PAYLOAD_LEN
            && self.data[ARP_HTYPE_START..ARP_PTYPE_START] == ARP_HTYPE
            && self.data[ARP_PTYPE_START..ARP_HLEN_OFF] == ARP_PTYPE
            && self.data[ARP_HLEN_OFF] == ARP_HLEN
            && self.data[ARP_PLEN_OFF] == ARP_PLEN
    }
    pub fn operation(&self) -> [u8; 2] {
        [self.data[ARP_OP_START], self.data[ARP_OP_START + 1]]
    }
    pub fn is_reply(&self) -> bool {
        self.operation() == ARP_OP_REPLY
    }
    pub fn is_request(&self) -> bool {
        self.operation() == ARP_OP_REQUEST
    }
    pub fn sender_mac(&self) -> &[u8] {
        &self.data[ARP_SENDER_MAC_START..ARP_SENDER_IP_START]
    }
    pub fn sender_ip(&self) -> [u8; 4] {
        [
            self.data[ARP_SENDER_IP_START],
            self.data[ARP_SENDER_IP_START + 1],
            self.data[ARP_SENDER_IP_START + 2],
            self.data[ARP_SENDER_IP_START + 3],
        ]
    }
    pub fn target_ip(&self) -> [u8; 4] {
        [
            self.data[ARP_TARGET_IP_START],
            self.data[ARP_TARGET_IP_START + 1],
            self.data[ARP_TARGET_IP_START + 2],
            self.data[ARP_TARGET_IP_START + 3],
        ]
    }
}

// =============================================================================
// Funciones de escritura
// =============================================================================

/// Escribe el payload ARP (28 bytes) en `buf[ETH_HEADER_SIZE..]`.
fn write_arp_payload(
    buf: &mut [u8],
    operation: &[u8; 2],
    sender_mac: &[u8],
    sender_ip: &[u8],
    target_mac: &[u8],
    target_ip: &[u8],
) {
    let p = &mut buf[ETH_HEADER_SIZE..];
    p[ARP_HTYPE_START..ARP_PTYPE_START].copy_from_slice(&ARP_HTYPE);
    p[ARP_PTYPE_START..ARP_HLEN_OFF].copy_from_slice(&ARP_PTYPE);
    p[ARP_HLEN_OFF] = ARP_HLEN;
    p[ARP_PLEN_OFF] = ARP_PLEN;
    p[ARP_OP_START..ARP_SENDER_MAC_START].copy_from_slice(operation);
    p[ARP_SENDER_MAC_START..ARP_SENDER_IP_START].copy_from_slice(sender_mac);
    p[ARP_SENDER_IP_START..ARP_TARGET_MAC_START].copy_from_slice(sender_ip);
    p[ARP_TARGET_MAC_START..ARP_TARGET_IP_START].copy_from_slice(target_mac);
    p[ARP_TARGET_IP_START..ARP_TARGET_IP_START + ARP_PLEN as usize].copy_from_slice(target_ip);
}

/// Escribe un frame ARP Reply completo (42 bytes) en `buf`.
pub fn write_arp_reply_frame(
    buf: &mut [u8],
    our_mac: &[u8],
    our_ip: [u8; 4],
    target_mac: &[u8],
    target_ip: [u8; 4],
) {
    write_eth_header(
        &mut buf[..ETH_HEADER_SIZE],
        target_mac,
        our_mac,
        ETH_TYPE_ARP,
    );
    write_arp_payload(buf, &ARP_OP_REPLY, our_mac, &our_ip, target_mac, &target_ip);
}

/// Escribe un frame ARP Request completo (42 bytes) en `buf`.
pub fn write_arp_request_frame(
    buf: &mut [u8],
    our_mac: &[u8],
    our_ip: [u8; 4],
    target_ip: [u8; 4],
) {
    write_eth_header(
        &mut buf[..ETH_HEADER_SIZE],
        &MAC_BROADCAST,
        our_mac,
        ETH_TYPE_ARP,
    );
    write_arp_payload(
        buf,
        &ARP_OP_REQUEST,
        our_mac,
        &our_ip,
        &MAC_ZERO,
        &target_ip,
    );
}

// =============================================================================
// Caché ARP — array ordenado por IP con búsqueda binaria
// =============================================================================

const MAX_ARP_ENTRIES: usize = 64;

#[derive(Clone, Copy)]
pub struct ArpEntry {
    pub ip: [u8; 4],
    pub mac: [u8; 6],
    pub inserted_at: Instant,
}

pub struct ArpCache {
    entries: [ArpEntry; MAX_ARP_ENTRIES],
    len: usize,
}

fn binary_search(entries: &[ArpEntry], ip: [u8; 4]) -> Option<usize> {
    let mut low = 0usize;
    let mut high = entries.len();
    while low < high {
        let mid = low + (high - low) / 2;
        if entries[mid].ip == ip {
            return Some(mid);
        } else if entries[mid].ip < ip {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    None
}

impl ArpCache {
    pub fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| ArpEntry {
                ip: [0; 4],
                mac: [0; 6],
                inserted_at: Instant::now(),
            }),
            len: 0,
        }
    }

    /// Busca una MAC por IP. Búsqueda binaria — O(log n).
    pub fn lookup(&self, ip: [u8; 4]) -> Option<[u8; 6]> {
        binary_search(&self.entries[..self.len], ip).map(|i| self.entries[i].mac)
    }

    /// Inserta o actualiza la entrada para `ip`. Mantiene el array ordenado.
    /// Devuelve `false` si la caché está llena y la IP es nueva.
    pub fn insert(&mut self, ip: [u8; 4], mac: [u8; 6]) -> bool {
        let slice = &self.entries[..self.len];
        if let Some(i) = binary_search(slice, ip) {
            self.entries[i].mac = mac;
            self.entries[i].inserted_at = Instant::now();
            return true;
        }
        let pos = if self.len >= MAX_ARP_ENTRIES {
            self.len - 1
        } else {
            let p = self.len;
            self.len += 1;
            p
        };
        self.entries[pos] = ArpEntry {
            ip,
            mac,
            inserted_at: Instant::now(),
        };
        self.entries[..self.len].sort_by_key(|e| e.ip);
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Elimina las entradas cuya antigüedad supera `max_age`.
    pub fn expire_old(&mut self, max_age: Duration) {
        let now = Instant::now();
        let mut i = 0;
        while i < self.len {
            if now.duration_since(self.entries[i].inserted_at) > max_age {
                self.entries.copy_within(i + 1..self.len, i);
                self.len -= 1;
            } else {
                i += 1;
            }
        }
    }

    pub fn entries(&self) -> &[ArpEntry] {
        &self.entries[..self.len]
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const IP1: [u8; 4] = [10, 0, 1, 1];
    const IP2: [u8; 4] = [10, 0, 1, 2];
    const IP3: [u8; 4] = [10, 0, 1, 3];
    const MAC1: [u8; 6] = [0xAA, 0, 0, 0, 0, 1];
    const MAC2: [u8; 6] = [0xAA, 0, 0, 0, 0, 2];
    const MAC3: [u8; 6] = [0xAA, 0, 0, 0, 0, 3];

    #[test]
    fn lookup_vacia_devuelve_none() {
        let cache = ArpCache::new();
        assert!(cache.lookup(IP1).is_none());
    }

    #[test]
    fn insert_y_lookup() {
        let mut cache = ArpCache::new();
        cache.insert(IP1, MAC1);
        assert_eq!(cache.lookup(IP1), Some(MAC1));
    }

    #[test]
    fn lookup_ip_inexistente() {
        let mut cache = ArpCache::new();
        cache.insert(IP1, MAC1);
        assert!(cache.lookup(IP2).is_none());
    }

    #[test]
    fn insert_multiples_orden_correcto() {
        let mut cache = ArpCache::new();
        // Insertamos en orden inverso
        cache.insert(IP3, MAC3);
        cache.insert(IP1, MAC1);
        cache.insert(IP2, MAC2);

        // La caché debe estar ordenada por IP
        let entries = cache.entries();
        assert_eq!(entries[0].ip, IP1);
        assert_eq!(entries[1].ip, IP2);
        assert_eq!(entries[2].ip, IP3);
    }

    #[test]
    fn lookup_binario_todos() {
        let mut cache = ArpCache::new();
        cache.insert(IP3, MAC3);
        cache.insert(IP1, MAC1);
        cache.insert(IP2, MAC2);

        assert_eq!(cache.lookup(IP1), Some(MAC1));
        assert_eq!(cache.lookup(IP2), Some(MAC2));
        assert_eq!(cache.lookup(IP3), Some(MAC3));
    }

    #[test]
    fn actualiza_mac_existente() {
        let mut cache = ArpCache::new();
        cache.insert(IP1, MAC1);
        let mac_nueva = [0xBB, 0, 0, 0, 0, 1];
        cache.insert(IP1, mac_nueva);
        assert_eq!(cache.lookup(IP1), Some(mac_nueva));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn arp_request_frame_broadcast() {
        let our_mac = [0xAA, 0, 0, 0, 0, 1];
        let our_ip = [10, 0, 1, 1];
        let target_ip = [10, 0, 1, 2];
        let mut buf = [0u8; ARP_FRAME_SIZE];
        write_arp_request_frame(&mut buf, &our_mac, our_ip, target_ip);
        // Destino Ethernet debe ser broadcast
        assert_eq!(&buf[0..6], &[0xFF; 6]);
        // Operación = Request
        assert_eq!(&buf[20..22], &ARP_OP_REQUEST);
        // Target IP
        assert_eq!(&buf[38..42], &target_ip);
    }

    #[test]
    fn arp_reply_frame_correcto() {
        let our_mac = [0xAA, 0, 0, 0, 0, 1];
        let our_ip = [10, 0, 1, 1];
        let target_mac = [0xBB, 0, 0, 0, 0, 2];
        let target_ip = [10, 0, 1, 2];
        let mut buf = [0u8; ARP_FRAME_SIZE];
        write_arp_reply_frame(&mut buf, &our_mac, our_ip, &target_mac, target_ip);
        // Destino Ethernet = target_mac
        assert_eq!(&buf[0..6], &target_mac);
        // Operación = Reply
        assert_eq!(&buf[20..22], &ARP_OP_REPLY);
        // Sender MAC = our_mac
        assert_eq!(&buf[22..28], &our_mac);
    }
}
