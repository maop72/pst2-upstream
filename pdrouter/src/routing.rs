// routing — Tabla de encaminamiento IP.
//
// Cada entrada tiene: red destino, máscara, next-hop (None = directamente
// conectado) e interfaz de salida.
//
// Las entradas se mantienen ordenadas de máscara más específica (más bits a 1)
// a menos específica. La búsqueda es lineal: el primer match (AND(dst,mask)==net)
// es el más específico, así se implementa Longest Prefix Match sin ordenación
// adicional por dirección.

const MAX_ROUTES: usize = 32;

// =============================================================================
// Tipos
// =============================================================================

#[derive(Clone, Copy)]
pub struct RouteEntry {
    pub network: [u8; 4],
    pub mask: [u8; 4],
    /// None = directamente conectado (no hay gateway).
    pub next_hop: Option<[u8; 4]>,
    pub iface: [u8; 16], // nombre de la interfaz, p.ej. "eth0\0..."
}

pub struct RoutingTable {
    entries: [Option<RouteEntry>; MAX_ROUTES],
    len: usize,
}

// =============================================================================
// Helpers
// =============================================================================

/// Cuenta los bits a 1 de una máscara de red (popcount).
fn mask_bits(mask: [u8; 4]) -> u32 {
    u32::from_be_bytes(mask).count_ones()
}

/// Aplica la máscara a una dirección IP.
fn apply_mask(ip: [u8; 4], mask: [u8; 4]) -> [u8; 4] {
    [
        ip[0] & mask[0],
        ip[1] & mask[1],
        ip[2] & mask[2],
        ip[3] & mask[3],
    ]
}

pub fn iface_name(s: &str) -> [u8; 16] {
    let mut arr = [0u8; 16];
    let bytes = s.as_bytes();
    let len = bytes.len().min(16);
    arr[..len].copy_from_slice(&bytes[..len]);
    arr
}

pub fn iface_str(arr: &[u8; 16]) -> &str {
    let mut end = 16;
    for i in 0..16 {
        if arr[i] == 0 {
            end = i;
            break;
        }
    }
    std::str::from_utf8(&arr[..end]).unwrap_or("")
}

// =============================================================================
// Implementación
// =============================================================================

impl RoutingTable {
    pub fn new() -> Self {
        Self {
            entries: [None; MAX_ROUTES],
            len: 0,
        }
    }

    /// Inserta una entrada manteniendo el orden por máscara descendente.
    /// Devuelve `false` si la tabla está llena.
    pub fn insert(&mut self, entry: RouteEntry) -> bool {
        if self.len >= MAX_ROUTES {
            return false;
        }
        // Posición de inserción: primera entrada con menos bits que la nueva
        let new_bits = mask_bits(entry.mask);
        let mut pos = self.len;
        for i in 0..self.len {
            if mask_bits(self.entries[i].as_ref().unwrap().mask) < new_bits {
                pos = i;
                break;
            }
        }

        // Desplazar hacia la derecha desde pos
        for i in (pos..self.len).rev() {
            self.entries[i + 1] = self.entries[i];
        }
        self.entries[pos] = Some(entry);
        self.len += 1;
        true
    }

    /// Elimina la primera entrada que coincida exactamente con network+mask.
    /// Devuelve `true` si se encontró y eliminó.
    pub fn remove(&mut self, network: [u8; 4], mask: [u8; 4]) -> bool {
        for i in 0..self.len {
            if let Some(e) = &self.entries[i] {
                if e.network == network && e.mask == mask {
                    // Desplazar hacia la izquierda
                    for j in i..self.len - 1 {
                        self.entries[j] = self.entries[j + 1];
                    }
                    self.entries[self.len - 1] = None;
                    self.len -= 1;
                    return true;
                }
            }
        }
        false
    }

    /// Búsqueda Longest Prefix Match: recorre de más específico a menos.
    /// Devuelve referencia a la primera entrada donde AND(dst, mask) == network.
    pub fn lookup(&self, dst: [u8; 4]) -> Option<&RouteEntry> {
        for i in 0..self.len {
            if let Some(e) = &self.entries[i] {
                if apply_mask(dst, e.mask) == e.network {
                    return Some(e);
                }
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Devuelve el slice de entradas activas (todas son Some).
    pub fn active_entries(&self) -> &[Option<RouteEntry>] {
        &self.entries[..self.len]
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(net: [u8; 4], mask: [u8; 4], nh: Option<[u8; 4]>, iface: &str) -> RouteEntry {
        RouteEntry {
            network: net,
            mask,
            next_hop: nh,
            iface: iface_name(iface),
        }
    }

    #[test]
    fn insert_ordenado_por_mascara() {
        let mut t = RoutingTable::new();
        t.insert(entry([0, 0, 0, 0], [0, 0, 0, 0], None, "eth0")); // /0 default
        t.insert(entry([10, 0, 1, 0], [255, 255, 255, 0], None, "eth0")); // /24
        t.insert(entry([10, 0, 0, 0], [255, 255, 0, 0], None, "eth1")); // /16

        let entries = t.active_entries();
        assert_eq!(mask_bits(entries[0].as_ref().unwrap().mask), 24);
        assert_eq!(mask_bits(entries[1].as_ref().unwrap().mask), 16);
        assert_eq!(mask_bits(entries[2].as_ref().unwrap().mask), 0);
    }

    #[test]
    fn lookup_longest_prefix_match() {
        let mut t = RoutingTable::new();
        t.insert(entry([10, 0, 0, 0], [255, 0, 0, 0], None, "eth1")); // /8
        t.insert(entry([10, 0, 1, 0], [255, 255, 255, 0], None, "eth0")); // /24

        let r = t.lookup([10, 0, 1, 5]).unwrap();
        // Debe elegir la /24, no la /8
        assert_eq!(mask_bits(r.mask), 24);
        assert_eq!(iface_str(&r.iface), "eth0");
    }

    #[test]
    fn lookup_default_route() {
        let mut t = RoutingTable::new();
        t.insert(entry(
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            Some([10, 0, 1, 254]),
            "eth0",
        ));
        let r = t.lookup([8, 8, 8, 8]).unwrap();
        assert_eq!(r.next_hop, Some([10, 0, 1, 254]));
    }

    #[test]
    fn lookup_sin_ruta_devuelve_none() {
        let mut t = RoutingTable::new();
        t.insert(entry([10, 0, 1, 0], [255, 255, 255, 0], None, "eth0"));
        assert!(t.lookup([192, 168, 1, 1]).is_none());
    }

    #[test]
    fn remove_elimina_entrada() {
        let mut t = RoutingTable::new();
        t.insert(entry([10, 0, 1, 0], [255, 255, 255, 0], None, "eth0"));
        assert_eq!(t.len(), 1);
        let removed = t.remove([10, 0, 1, 0], [255, 255, 255, 0]);
        assert!(removed);
        assert_eq!(t.len(), 0);
        assert!(t.lookup([10, 0, 1, 1]).is_none());
    }

    #[test]
    fn remove_inexistente_devuelve_false() {
        let mut t = RoutingTable::new();
        assert!(!t.remove([10, 0, 1, 0], [255, 255, 255, 0]));
    }

    #[test]
    fn tabla_llena_devuelve_false() {
        let mut t = RoutingTable::new();
        for i in 0..MAX_ROUTES as u8 {
            let ok = t.insert(entry([10, 0, 0, i], [255, 255, 255, 255], None, "eth0"));
            assert!(ok);
        }
        let ok = t.insert(entry([10, 0, 1, 0], [255, 255, 255, 0], None, "eth0"));
        assert!(!ok);
    }

    #[test]
    fn lookup_exact_match() {
        let mut t = RoutingTable::new();
        t.insert(entry([10, 0, 1, 5], [255, 255, 255, 255], None, "eth0")); // /32
        let r = t.lookup([10, 0, 1, 5]).unwrap();
        assert_eq!(mask_bits(r.mask), 32);
    }
}
