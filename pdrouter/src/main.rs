// router — Pila IP con encaminamiento sobre interfaces Ethernet.
//
// Arranca leyendo /etc/network/interfaces, construye la tabla de rutas
// inicial y lanza el hilo de red. La interfaz de usuario (UI) corre en
// el hilo principal.

mod arp;
mod eth;
mod frame;
mod icmp;
mod interfaces;
mod ipv4;
mod routing;
mod stack;
mod tui;
mod ui;
mod utils;

use interfaces::parse_interfaces;
use stack::run_stack;
use ui::run_ui;

use std::sync::mpsc;
use std::thread;

fn main() {
    let content = std::fs::read_to_string("/etc/network/interfaces")
        .expect("No se pudo leer /etc/network/interfaces");

    let parsed = parse_interfaces(&content);

    if parsed.ifaces_count == 0 {
        eprintln!("Error: no se encontraron interfaces con IP en /etc/network/interfaces");
        std::process::exit(1);
    }

    let (ui_to_stack_tx, ui_to_stack_rx) = mpsc::channel::<stack::Command>();
    let (stack_to_ui_tx, stack_to_ui_rx) = mpsc::channel::<stack::Event>();

    let iface_configs = parsed.ifaces;
    let iface_count = parsed.ifaces_count;
    let initial_routes = parsed.routes;
    let routes_count = parsed.routes_count;

    // Creamos hilo para el código de la pila de la red
    let _stack = thread::spawn(move || {
        run_stack(
            &iface_configs[..iface_count],
            &initial_routes[..routes_count],
            ui_to_stack_rx,
            stack_to_ui_tx,
        );
    });

    // En este hilo principal corre la interfaz de usuario (UI)
    run_ui(ui_to_stack_tx, stack_to_ui_rx);
}
