// interfaces — Lectura de /etc/network/interfaces y descubrimiento de interfaces.
//
// Parsea el fichero de configuración de red de Debian/Ubuntu para extraer:
//   - Dirección IP y máscara de cada interfaz
//   - Rutas estáticas (up ip route add ...)
//   - Gateway por defecto (gateway)
//
// Formato esperado (subconjunto relevante):
//
//   auto lo
//   iface lo inet loopback
//
//   auto eth0
//   iface eth0 inet static
//       address 10.0.1.1
//       netmask 255.255.255.0
//       gateway 10.0.1.254        ← opcional
//       up ip route add 10.0.3.0/24 via 10.0.2.2   ← opcional, múltiples
//
// Resultado: arrays fijos con la información de cada interfaz.

use crate::routing::{iface_name, RouteEntry};
use crate::utils::{parse_ipv4, parse_mask};

pub const MAX_IFACES: usize = 8;
pub const MAX_CONFIG_ROUTES: usize = 64;

// =============================================================================
// Tipos
// =============================================================================

#[derive(Clone, Copy)]
pub struct InterfaceConfig {
    pub name: [u8; 16],
    pub ip: [u8; 4],
    pub mask: [u8; 4],
}

// =============================================================================
// Parseo de /etc/network/interfaces
// =============================================================================

/// Resultado del parseo de un fichero de interfaces.
pub struct ParsedInterfaces {
    pub ifaces: [InterfaceConfig; MAX_IFACES],
    pub ifaces_count: usize,
    pub routes: [RouteEntry; MAX_CONFIG_ROUTES],
    pub routes_count: usize,
}

/// Parsea el contenido de un fichero /etc/network/interfaces.
pub fn parse_interfaces(content: &str) -> ParsedInterfaces {
    let empty_iface = InterfaceConfig { name: [0; 16], ip: [0; 4], mask: [0; 4] };
    let empty_route = RouteEntry { network: [0; 4], mask: [0; 4], next_hop: None, iface: [0; 16] };
    let mut result = ParsedInterfaces {
        ifaces: [empty_iface; MAX_IFACES],
        ifaces_count: 0,
        routes: [empty_route; MAX_CONFIG_ROUTES],
        routes_count: 0,
    };

    // Estado del parser
    let mut current_name: Option<[u8; 16]> = None;
    let mut current_ip: Option<[u8; 4]> = None;
    let mut current_mask: Option<[u8; 4]> = None;

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // "iface <nombre> inet static|loopback|dhcp"
        if let Some(rest) = line.strip_prefix("iface ") {
            // Guardar la interfaz anterior si estaba completa
            if let (Some(n), Some(i), Some(m)) = (current_name, current_ip, current_mask) {
                if result.ifaces_count < MAX_IFACES {
                    result.ifaces[result.ifaces_count] = InterfaceConfig { name: n, ip: i, mask: m };
                    result.ifaces_count += 1;
                }
            }
            current_ip = None;
            current_mask = None;

            let mut parts = rest.split_whitespace();
            current_name = parts.next().map(iface_name);
            continue;
        }

        // Propiedades de la interfaz actual
        if let Some(name) = current_name {
            if let Some(addr_str) = line.strip_prefix("address ") {
                current_ip = parse_ipv4(addr_str.trim()).ok();
            } else if let Some(mask_str) = line.strip_prefix("netmask ") {
                current_mask = parse_mask(mask_str.trim()).ok();
            } else if let Some(gw_str) = line.strip_prefix("gateway ") {
                // Ruta por defecto: 0.0.0.0/0 via gateway
                if let Ok(gw) = parse_ipv4(gw_str.trim()) {
                    if result.routes_count < MAX_CONFIG_ROUTES {
                        result.routes[result.routes_count] = RouteEntry {
                            network: [0, 0, 0, 0],
                            mask: [0, 0, 0, 0],
                            next_hop: Some(gw),
                            iface: name,
                        };
                        result.routes_count += 1;
                    }
                }
            } else if let Some(rest) = line.strip_prefix("up ip route add ") {
                // "up ip route add <net>/<prefix> via <gw>"
                if let Some(route) = parse_up_route(rest.trim(), name) {
                    if result.routes_count < MAX_CONFIG_ROUTES {
                        result.routes[result.routes_count] = route;
                        result.routes_count += 1;
                    }
                }
            }
        }
    }

    // Guardar la última interfaz
    if let (Some(n), Some(i), Some(m)) = (current_name, current_ip, current_mask) {
        if result.ifaces_count < MAX_IFACES {
            result.ifaces[result.ifaces_count] = InterfaceConfig { name: n, ip: i, mask: m };
            result.ifaces_count += 1;
        }
    }

    // Añadir rutas de red directamente conectadas para cada interfaz con IP+mask
    for k in 0..result.ifaces_count {
        let iface = &result.ifaces[k];
        let network = apply_mask(iface.ip, iface.mask);
        if result.routes_count < MAX_CONFIG_ROUTES {
            result.routes[result.routes_count] = RouteEntry {
                network,
                mask: iface.mask,
                next_hop: None, // directamente conectada
                iface: iface.name,
            };
            result.routes_count += 1;
        }
    }

    result
}

/// Parsea una línea "10.0.3.0/24 via 10.0.2.2" en un RouteEntry.
fn parse_up_route(s: &str, iface: [u8; 16]) -> Option<RouteEntry> {
    // Formato: "<net>/<mask> via <gw>"
    let (net_part, rest) = s.split_once(" via ")?;
    let gw = parse_ipv4(rest.trim()).ok()?;

    let (net_str, mask_str) = if let Some((n, m)) = net_part.split_once('/') {
        (n, format!("/{m}"))
    } else {
        return None;
    };

    let network = parse_ipv4(net_str.trim()).ok()?;
    let mask = parse_mask(mask_str.trim()).ok()?;
    let network = apply_mask(network, mask);

    Some(RouteEntry {
        network,
        mask,
        next_hop: Some(gw),
        iface,
    })
}

fn apply_mask(ip: [u8; 4], mask: [u8; 4]) -> [u8; 4] {
    [ip[0] & mask[0], ip[1] & mask[1], ip[2] & mask[2], ip[3] & mask[3]]
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::iface_str;

    const SIMPLE: &str = "
auto eth0
iface eth0 inet static
    address 10.0.1.1
    netmask 255.255.255.0
    gateway 10.0.1.254
";

    const MULTI: &str = "
auto lo
iface lo inet loopback

auto eth0
iface eth0 inet static
    address 10.0.1.1
    netmask 255.255.255.0
    gateway 10.0.1.254
    up ip route add 10.0.3.0/24 via 10.0.2.2

auto eth1
iface eth1 inet static
    address 10.0.2.1
    netmask 255.255.255.0
";

    #[test]
    fn parsea_ip_y_mask() {
        let r = parse_interfaces(SIMPLE);
        assert_eq!(r.ifaces_count, 1);
        assert_eq!(r.ifaces[0].ip, [10, 0, 1, 1]);
        assert_eq!(r.ifaces[0].mask, [255, 255, 255, 0]);
    }

    #[test]
    fn parsea_nombre_iface() {
        let r = parse_interfaces(SIMPLE);
        assert_eq!(iface_str(&r.ifaces[0].name), "eth0");
    }

    #[test]
    fn genera_ruta_directamente_conectada() {
        let r = parse_interfaces(SIMPLE);
        let mut connected_count = 0usize;
        let mut connected_idx = 0usize;
        for k in 0..r.routes_count {
            if r.routes[k].next_hop.is_none() {
                if connected_count == 0 { connected_idx = k; }
                connected_count += 1;
            }
        }
        assert_eq!(connected_count, 1);
        assert_eq!(r.routes[connected_idx].network, [10, 0, 1, 0]);
        assert_eq!(r.routes[connected_idx].mask, [255, 255, 255, 0]);
    }

    #[test]
    fn genera_ruta_default_desde_gateway() {
        let r = parse_interfaces(SIMPLE);
        let mut default_count = 0usize;
        let mut default_idx = 0usize;
        for k in 0..r.routes_count {
            if r.routes[k].mask == [0, 0, 0, 0] {
                if default_count == 0 { default_idx = k; }
                default_count += 1;
            }
        }
        assert_eq!(default_count, 1);
        assert_eq!(r.routes[default_idx].next_hop, Some([10, 0, 1, 254]));
    }

    #[test]
    fn parsea_up_route() {
        let r = parse_interfaces(MULTI);
        let mut static_count = 0usize;
        let mut static_idx = 0usize;
        for k in 0..r.routes_count {
            if r.routes[k].network == [10, 0, 3, 0] && r.routes[k].mask == [255, 255, 255, 0] {
                if static_count == 0 { static_idx = k; }
                static_count += 1;
            }
        }
        assert_eq!(static_count, 1);
        assert_eq!(r.routes[static_idx].next_hop, Some([10, 0, 2, 2]));
    }

    #[test]
    fn parsea_multiples_interfaces() {
        let r = parse_interfaces(MULTI);
        // lo no tiene address+netmask, así que no se añade a ifaces
        assert_eq!(r.ifaces_count, 2);
    }

    #[test]
    fn lineas_vacias_y_comentarios_ignorados() {
        let content = "
# comentario
auto eth0
iface eth0 inet static
    # otro comentario
    address 192.168.1.1
    netmask 255.255.0.0
";
        let r = parse_interfaces(content);
        assert_eq!(r.ifaces[0].ip, [192, 168, 1, 1]);
        assert_eq!(r.ifaces[0].mask, [255, 255, 0, 0]);
    }
}
