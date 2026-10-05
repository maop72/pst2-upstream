// ui — Bucle de interfaz de usuario del router.
//
// Comandos disponibles:
//   help
//   route
//   show arp
//   ping <ip>
//   add route <net>/<prefix> [via <gw>] dev <iface>
//   del route <net>/<prefix>
//   quit / exit

use crate::stack::{Command, Event};
use crate::tui::Tui;
use crate::utils::{format_ip, parse_ipv4, parse_mask};

use std::sync::mpsc;

const HELP: &str = "\
Comandos disponibles:
  route                                        Muestra la tabla de rutas
  arp                                          Muestra la caché ARP
  ping <ip>                                    Envía un ICMP Echo Request
  add route <red>/<prefijo> [via <gw>] dev <iface>  Añade una ruta
  del route <red>/<prefijo>                    Elimina una ruta
  quit / exit                                  Apaga el router";

pub fn run_ui(ui_tx: mpsc::Sender<Command>, ui_rx: mpsc::Receiver<Event>) {
    let mut tui = Tui::new();
    tui.print("Router listo. Escribe 'help' para ver los comandos disponibles.");

    loop {
        // Vuelca eventos del stack al log
        while let Ok(evt) = ui_rx.try_recv() {
            let msg = match evt {
                Event::Log(s) => s,
                Event::PingReply { from } => format!("Respuesta de {} recibida", format_ip(from)),
                Event::RouteDump(s) => {
                    if s.is_empty() {
                        "Tabla de rutas vacía".to_string()
                    } else {
                        s
                    }
                }
                Event::ArpDump(s) => {
                    if s.is_empty() {
                        "Caché ARP vacía".to_string()
                    } else {
                        format!("{:<16}{}\n{}", "IP", "MAC", s)
                    }
                }
            };
            tui.print(&msg);
        }

        let Some(line) = tui.read_line() else {
            continue;
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        if line == "quit" || line == "exit" {
            let _ = ui_tx.send(Command::Quit);
            break;
        }

        if line == "help" {
            tui.print(HELP);
            continue;
        }

        if line == "route" || line == "show routes" {
            let _ = ui_tx.send(Command::ShowRoutes);
            continue;
        }

        if line == "arp" || line == "show arp" {
            let _ = ui_tx.send(Command::ShowArp);
            continue;
        }

        if let Some(rest) = line.strip_prefix("ping ") {
            match parse_ipv4(rest.trim()) {
                Ok(ip) => {
                    tui.print(&format!("Enviando ping a {}...", format_ip(ip)));
                    let _ = ui_tx.send(Command::Ping(ip));
                }
                Err(e) => tui.print(&format!("IP inválida: {e}")),
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("add route ") {
            match parse_add_route(rest) {
                Ok(cmd) => {
                    let _ = ui_tx.send(cmd);
                }
                Err(e) => tui.print(&format!("Error: {e}")),
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("del route ") {
            match parse_del_route(rest) {
                Ok(cmd) => {
                    let _ = ui_tx.send(cmd);
                }
                Err(e) => tui.print(&format!("Error: {e}")),
            }
            continue;
        }

        tui.print("Comando desconocido. Escribe 'help' para ver los comandos disponibles.");
    }
}

// =============================================================================
// Parseo de comandos
// =============================================================================

/// Parsea "10.0.3.0/24 [via 10.0.2.2] dev eth0"
fn parse_add_route(s: &str) -> Result<Command, String> {
    // Separar "dev <iface>" al final
    let (prefix_part, iface) = s.rsplit_once(" dev ").ok_or("falta 'dev <iface>'")?;
    let iface = iface.trim().to_string();

    // ¿Hay "via"?
    let (net_part, next_hop) = if let Some((net, gw)) = prefix_part.split_once(" via ") {
        let gw_ip = parse_ipv4(gw.trim())?;
        (net.trim(), Some(gw_ip))
    } else {
        (prefix_part.trim(), None)
    };

    // Parsear red/máscara
    let (network, mask) = parse_net_mask(net_part)?;

    Ok(Command::AddRoute {
        network,
        mask,
        next_hop,
        iface,
    })
}

/// Parsea "10.0.3.0/24"
fn parse_del_route(s: &str) -> Result<Command, String> {
    let (network, mask) = parse_net_mask(s.trim())?;
    Ok(Command::DelRoute { network, mask })
}

/// Parsea "<net>/<prefix|mask>" → (network, mask)
fn parse_net_mask(s: &str) -> Result<([u8; 4], [u8; 4]), String> {
    let (net_str, mask_str) = s
        .split_once('/')
        .ok_or("formato esperado: <red>/<máscara>")?;
    let network = parse_ipv4(net_str.trim())?;
    let mask = parse_mask(&format!("/{}", mask_str.trim()))?;
    Ok((network, mask))
}
