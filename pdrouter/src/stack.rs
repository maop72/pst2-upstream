// stack — Código principal del router que ejecuta el hilo de la red, que llama
// a fn run_stack() desde main()
//
// Gestiona todas las interfaces Ethernet simultáneamente (poll con timeout 1ms
// igual que el switch). Para cada frame recibido:
//
//   1. ARP → responder si la IP destino es alguna de las nuestras, y actualizar
//      la caché ARP con el sender.
//   2. IPv4 con dst = una de nuestras IPs → procesar localmente (ICMP echo reply).
//   3. IPv4 con dst ajena → forwarding:
//        a. Decrementar TTL; si llega a 0, enviar ICMP Time Exceeded.
//        b. Lookup en tabla de rutas; si no hay ruta, ICMP Net Unreachable.
//        c. Resolver MAC del next-hop vía ARP; si no está, encolar y lanzar ARP Request.
//        d. Reenviar por la interfaz correcta.
//
// Los mensajes ICMP de error se encaminan siempre usando la tabla de rutas,
// igual que cualquier otro datagrama IP pues cada mensaje ICMP se encapsula
// en un datagrama IP. Así un encaminador intermedio puede mensajes ICMPs de erro
// al origen, aunque el origen no esté en ninguna de las redes a las que está
// directamente conectado el router.
//
// Canales para comunicación entre los 2 hilos:
//   Command (UI → Stack): Ping, AddRoute, DelRoute, ShowRoutes, ShowArp, Quit
//   Event   (Stack → UI): Log, PingReply, RouteDump, ArpDump

use std::sync::mpsc;

use pnet::datalink::{self, Channel::Ethernet, Config, DataLinkReceiver, DataLinkSender};
use std::time::{Duration, Instant};

use crate::frame::Frame;

use crate::arp::{
    write_arp_reply_frame, write_arp_request_frame, ArpCache, ArpView, ARP_FRAME_SIZE,
    ARP_PAYLOAD_LEN,
};
use crate::eth::{EthView, ETH_HEADER_SIZE, MAX_FRAME_SIZE};
use crate::icmp::{
    handle_echo_reply, handle_echo_request, write_icmp_echo_request_frame,
    write_icmp_host_unreachable_frame, write_icmp_net_unreachable_frame,
    write_icmp_time_exceeded_frame, ICMP_ECHO_FRAME_LEN,
};
use crate::interfaces::InterfaceConfig;
use crate::ipv4::{decrement_ttl_and_recompute_checksum, Ipv4Header, IP_HEADER_MIN_LEN};
use crate::routing::{iface_name, iface_str, RouteEntry, RoutingTable};
use crate::utils::{format_ip, format_mac};

const READ_TIMEOUT_MS: u64 = 1;
const ARP_TIMEOUT: Duration = Duration::from_millis(1000);
const ARP_ENTRY_TTL: Duration = Duration::from_secs(30);
const MAX_PORTS: usize = 8;
const MAX_PENDING_ARP: usize = 16;

// =============================================================================
// Tipo de datos enviado por UI => Stack
// =============================================================================

pub enum Command {
    Ping([u8; 4]),
    AddRoute {
        network: [u8; 4],
        mask: [u8; 4],
        next_hop: Option<[u8; 4]>,
        iface: String,
    },
    DelRoute {
        network: [u8; 4],
        mask: [u8; 4],
    },
    ShowRoutes,
    ShowArp,
    Quit,
}

// =============================================================================
// Tipo de datos enviado por Stack => UI
// =============================================================================

pub enum Event {
    Log(String),
    PingReply { from: [u8; 4] },
    RouteDump(String),
    ArpDump(String),
}

/*  ELIMINADO  Lo llevamos a frame.rs
// =============================================================================
// Buffer para una trama Ethernet de tamaño fijo
// =============================================================================

#[derive(Clone, Copy)]
struct Frame {
    data: [u8; MAX_FRAME_SIZE],
    len: usize,
}

impl Frame {
    fn new() -> Self {
        Frame { data: [0u8; MAX_FRAME_SIZE], len: 0 }
    }

    fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }
}
*/

// =============================================================================
// Estado interno del hilo de red
// =============================================================================

/// Paquete en espera de resolución ARP.
#[derive(Clone, Copy)]
struct PendingPacket {
    frame: Frame,
    out_iface_idx: usize,
    enqueued_at: Instant,
}

/// Un puerto del router: tx + rx + configuración.
struct Port {
    tx: Box<dyn DataLinkSender>,
    rx: Box<dyn DataLinkReceiver>,
    ip: [u8; 4],
    #[allow(dead_code)]
    mask: [u8; 4],
    mac: [u8; 6],
    name: [u8; 16],
}

struct Stack {
    ports: [Option<Port>; MAX_PORTS],
    port_count: usize,
    routing: RoutingTable,
    arp_cache: ArpCache,
    pending_arp: [Option<([u8; 4], PendingPacket)>; MAX_PENDING_ARP],
    pending_arp_count: usize,
    pending_ping: Option<[u8; 4]>,
}

// =============================================================================
// Hilo que ejecuta el código de la pila de red (envío y recepción)
// =============================================================================

pub fn run_stack(
    iface_configs: &[InterfaceConfig],
    initial_routes: &[RouteEntry],
    ui_rx: mpsc::Receiver<Command>,
    ui_tx: mpsc::Sender<Event>,
) {
    let config = Config {
        read_timeout: Some(Duration::from_millis(READ_TIMEOUT_MS)),
        write_buffer_size: 4096,
        read_buffer_size: 4096,
        ..Default::default()
    };

    let mut ports: [Option<Port>; MAX_PORTS] = std::array::from_fn(|_| None);
    let mut port_count = 0usize;

    for k in 0..iface_configs.len() {
        let iface_cfg = &iface_configs[k];
        let iface_name_str = iface_str(&iface_cfg.name);
        let pnet_ifaces = datalink::interfaces();
        let mut found = None;
        for iface in pnet_ifaces {
            if iface.name == iface_name_str {
                found = Some(iface);
                break;
            }
        }
        let pnet_iface = match found {
            Some(i) => i,
            None => {
                let _ = ui_tx.send(Event::Log(format!(
                    "WARN: interfaz '{}' no encontrada, ignorando",
                    iface_name_str
                )));
                continue;
            }
        };
        let mac = match pnet_iface.mac {
            Some(m) => m.octets(),
            None => {
                let _ = ui_tx.send(Event::Log(format!(
                    "WARN: '{}' sin MAC, ignorando",
                    iface_name_str
                )));
                continue;
            }
        };
        match datalink::channel(&pnet_iface, config) {
            Ok(Ethernet(tx, rx)) => {
                if port_count < MAX_PORTS {
                    ports[port_count] = Some(Port {
                        tx,
                        rx,
                        ip: iface_cfg.ip,
                        mask: iface_cfg.mask,
                        mac,
                        name: iface_cfg.name,
                    });
                    port_count += 1;
                }
            }
            _ => {
                let _ = ui_tx.send(Event::Log(format!(
                    "ERROR: no se pudo abrir canal para '{}'",
                    iface_name_str
                )));
            }
        }
    }

    if port_count == 0 {
        let _ = ui_tx.send(Event::Log("ERROR: sin interfaces disponibles".to_string()));
        return;
    }

    let mut routing = RoutingTable::new();
    for k in 0..initial_routes.len() {
        routing.insert(initial_routes[k]);
    }

    let mut stack = Stack {
        ports,
        port_count,
        routing,
        arp_cache: ArpCache::new(),
        pending_arp: std::array::from_fn(|_| None),
        pending_arp_count: 0,
        pending_ping: None,
    };

    let _ = ui_tx.send(Event::Log(format!(
        "Router arrancado con {} interfaces",
        stack.port_count
    )));

    // Bucle principal
    loop {
        // Recepción de tramas Ethernet en los puertos del router
        for i in 0..stack.port_count {
            let packet = match stack.ports[i].as_mut().unwrap().rx.next() {
                Ok(data) => {
                    let mut f = Frame::new();
                    let len = data.len().min(MAX_FRAME_SIZE);
                    f.data[..len].copy_from_slice(&data[..len]);
                    f.len = len;
                    f
                }
                Err(_) => continue,
            };
            process_frame(&mut stack, &ui_tx, i, packet);
        }

        // En cada vuelta del bucle ha pasado el tiempo, por lo que comprobamos
        // si hay que hacer expirar alguna entrada de la caché de ARP
        stack.arp_cache.expire_old(ARP_ENTRY_TTL);
        check_arp_timeouts(&mut stack, &ui_tx);

        // Comprobamos si el hilo de la interfaz de usuario nos ha enviado algún
        // comando
        match ui_rx.try_recv() {
            Ok(Command::Quit) => break,
            Ok(cmd) => handle_command(&mut stack, &ui_tx, cmd),
            Err(_) => {}
        }
    }
}

// =============================================================================
// Procesamiento de frames
// =============================================================================

fn process_frame(stack: &mut Stack, ui_tx: &mpsc::Sender<Event>, in_port: usize, packet: Frame) {
    if packet.len < ETH_HEADER_SIZE {
        return;
    }
    let eth = EthView {
        data: packet.as_slice(),
    };

    // Descartar frames no dirigidos a nosotros (unicast a otro nodo)
    let dst_mac = eth.dst_mac();
    let broadcast = [0xffu8; 6];
    let our_mac = stack.ports[in_port].as_ref().unwrap().mac;
    if dst_mac != &our_mac && dst_mac != &broadcast {
        return;
    }

    // Hay dos niveles encima de Ethernet: ARP e IP. Despachamos al código
    // de uno u otro en función del campo tipo de protocolo (EtherType) de
    // la trama Ethernet
    if eth.is_arp() {
        process_arp(stack, ui_tx, in_port, packet.as_slice());
        return;
    }

    if eth.is_ipv4() && packet.len >= ETH_HEADER_SIZE + IP_HEADER_MIN_LEN {
        process_ipv4(stack, ui_tx, in_port, packet);
    }
}

fn process_arp(stack: &mut Stack, ui_tx: &mpsc::Sender<Event>, in_port: usize, packet: &[u8]) {
    let eth = EthView { data: packet };
    let payload = eth.payload();
    if payload.len() < ARP_PAYLOAD_LEN {
        return;
    }
    let arp = ArpView { data: payload };
    if !arp.is_valid() {
        return;
    }

    // Aprender el sender en la caché
    let sender_ip = arp.sender_ip();
    let s = arp.sender_mac();
    let sender_mac = [s[0], s[1], s[2], s[3], s[4], s[5]];
    stack.arp_cache.insert(sender_ip, sender_mac);

    // Entregar paquetes pendientes para este IP
    deliver_pending_arp(stack, ui_tx, sender_ip, sender_mac);

    if arp.is_request() {
        let target_ip = arp.target_ip();
        let mut port_idx_opt = None;
        for i in 0..stack.port_count {
            if stack.ports[i].as_ref().unwrap().ip == target_ip {
                port_idx_opt = Some(i);
                break;
            }
        }
        if let Some(port_idx) = port_idx_opt {
            let our_mac = stack.ports[port_idx].as_ref().unwrap().mac;
            let our_ip = stack.ports[port_idx].as_ref().unwrap().ip;
            let mut reply = [0u8; ARP_FRAME_SIZE];
            write_arp_reply_frame(&mut reply, &our_mac, our_ip, &sender_mac, sender_ip);
            let _ = stack.ports[in_port]
                .as_mut()
                .unwrap()
                .tx
                .send_to(&reply, None);
            let _ = ui_tx.send(Event::Log(format!(
                "ARP Request de {} → Reply ({})",
                format_ip(sender_ip),
                iface_str(&stack.ports[in_port].as_ref().unwrap().name)
            )));
        }
    }
}

fn process_ipv4(stack: &mut Stack, ui_tx: &mpsc::Sender<Event>, in_port: usize, packet: Frame) {
    let dst_ip = {
        let eth = EthView {
            data: packet.as_slice(),
        };
        let ip = Ipv4Header {
            data: eth.payload(),
        };
        ip.dst_ip()
    };

    // ¿La IP destino es alguna de las nuestras?
    let mut our_port_opt = None;
    for i in 0..stack.port_count {
        if stack.ports[i].as_ref().unwrap().ip == dst_ip {
            our_port_opt = Some(i);
            break;
        }
    }

    if let Some(port_idx) = our_port_opt {
        process_local(stack, ui_tx, in_port, port_idx, packet.as_slice());
        return;
    }

    forward_packet(stack, ui_tx, packet);
}

fn process_local(
    stack: &mut Stack,
    ui_tx: &mpsc::Sender<Event>,
    in_port: usize,
    our_port: usize,
    packet: &[u8],
) {
    let our_mac = stack.ports[our_port].as_ref().unwrap().mac;
    let our_ip = stack.ports[our_port].as_ref().unwrap().ip;

    if let Some((reply_data, reply_len)) = handle_echo_request(packet, &our_mac, our_ip) {
        let eth = EthView { data: packet };
        let ip = Ipv4Header {
            data: eth.payload(),
        };
        let _ = ui_tx.send(Event::Log(format!(
            "Ping de {} recibido (TTL={})",
            format_ip(ip.src_ip()),
            ip.ttl()
        )));
        let _ = stack.ports[in_port]
            .as_mut()
            .unwrap()
            .tx
            .send_to(&reply_data[..reply_len], None);
        return;
    }

    if let Some(pending_ip) = stack.pending_ping {
        if handle_echo_reply(packet, pending_ip) {
            stack.pending_ping = None;
            let _ = ui_tx.send(Event::PingReply { from: pending_ip });
        }
    }
}

fn forward_packet(stack: &mut Stack, ui_tx: &mpsc::Sender<Event>, mut packet: Frame) {
    let eth_payload_start = ETH_HEADER_SIZE;

    // Extraer los datos necesarios antes de mutar el buffer
    let (src_ip, dst_ip, orig_ip_header, orig_8_bytes, ttl) = {
        let eth = EthView {
            data: packet.as_slice(),
        };
        let ip = Ipv4Header {
            data: eth.payload(),
        };
        let src = ip.src_ip();
        let dst = ip.dst_ip();
        let hdr_len = ip.ihl_bytes().min(IP_HEADER_MIN_LEN);
        let mut hdr = [0u8; IP_HEADER_MIN_LEN];
        hdr[..hdr_len].copy_from_slice(&eth.payload()[..hdr_len]);
        let mut payload8 = [0u8; 8];
        let p8 = ip.first_8_payload_bytes();
        let p8_len = p8.len().min(8);
        payload8[..p8_len].copy_from_slice(&p8[..p8_len]);
        (src, dst, hdr, payload8, ip.ttl())
    };

    // TTL ya expirado al llegar (TTL=1 → tras decrement será 0)
    if ttl <= 1 {
        let _ = ui_tx.send(Event::Log(format!(
            "TTL Exceeded: {} → {} (TTL={})",
            format_ip(src_ip),
            format_ip(dst_ip),
            ttl
        )));
        send_icmp_error(
            stack,
            ui_tx,
            src_ip,
            &orig_ip_header,
            &orig_8_bytes,
            IcmpErrorKind::TimeExceeded,
        );
        return;
    }

    decrement_ttl_and_recompute_checksum(&mut packet.data[eth_payload_start..packet.len]);

    // Buscar ruta de salida
    let (out_port, next_hop) = match resolve_route(stack, dst_ip) {
        Some(x) => x,
        None => {
            let _ = ui_tx.send(Event::Log(format!(
                "Net Unreachable: {} → {} (TTL={}), sin ruta",
                format_ip(src_ip),
                format_ip(dst_ip),
                ttl
            )));
            send_icmp_error(
                stack,
                ui_tx,
                src_ip,
                &orig_ip_header,
                &orig_8_bytes,
                IcmpErrorKind::NetUnreachable,
            );
            return;
        }
    };

    // Actualizar src MAC del frame con nuestra MAC de salida
    let out_mac = stack.ports[out_port].as_ref().unwrap().mac;
    packet.data[6..12].copy_from_slice(&out_mac);

    let _ = ui_tx.send(Event::Log(format!(
        "{} → {} (TTL={}) via {} ({})",
        format_ip(src_ip),
        format_ip(dst_ip),
        ttl - 1,
        format_ip(next_hop),
        iface_str(&stack.ports[out_port].as_ref().unwrap().name)
    )));

    send_frame_with_arp(stack, packet, next_hop, out_port);
}

// =============================================================================
// Envío con resolución ARP
// =============================================================================

/// Envía `frame` al next-hop usando la caché ARP.
/// Si no hay entrada ARP, encola el frame y lanza ARP Request.
/// El frame debe tener [0..6] disponibles para la dst MAC y [6..12] ya con nuestra MAC.
fn send_frame_with_arp(stack: &mut Stack, mut frame: Frame, next_hop: [u8; 4], out_port: usize) {
    match stack.arp_cache.lookup(next_hop) {
        Some(dst_mac) => {
            frame.data[0..6].copy_from_slice(&dst_mac);
            let _ = stack.ports[out_port]
                .as_mut()
                .unwrap()
                .tx
                .send_to(frame.as_slice(), None);
        }
        None => {
            send_arp_request(stack, out_port, next_hop);
            if stack.pending_arp_count < MAX_PENDING_ARP {
                stack.pending_arp[stack.pending_arp_count] = Some((
                    next_hop,
                    PendingPacket {
                        frame,
                        out_iface_idx: out_port,
                        enqueued_at: Instant::now(),
                    },
                ));
                stack.pending_arp_count += 1;
            }
        }
    }
}

// =============================================================================
// Mensajes ICMP de error
// =============================================================================

enum IcmpErrorKind {
    TimeExceeded,
    NetUnreachable,
    HostUnreachable,
}

/// Genera un mensaje ICMP de error y lo encamina hacia `error_dst_ip` usando
/// la tabla de rutas (igual que cualquier otro paquete).
fn send_icmp_error(
    stack: &mut Stack,
    ui_tx: &mpsc::Sender<Event>,
    error_dst_ip: [u8; 4],
    orig_ip_header: &[u8; IP_HEADER_MIN_LEN],
    orig_8_bytes: &[u8; 8],
    kind: IcmpErrorKind,
) {
    let (out_port, next_hop) = match resolve_route(stack, error_dst_ip) {
        Some(x) => x,
        None => {
            let _ = ui_tx.send(Event::Log(format!(
                "Sin ruta para enviar error ICMP a {}",
                format_ip(error_dst_ip)
            )));
            return;
        }
    };

    let our_mac = stack.ports[out_port].as_ref().unwrap().mac;
    let our_ip = stack.ports[out_port].as_ref().unwrap().ip;

    // Construir el frame con dst MAC provisional [0;6]; se rellenará en send_frame_with_arp
    let arr = match kind {
        IcmpErrorKind::TimeExceeded => write_icmp_time_exceeded_frame(
            &our_mac,
            our_ip,
            &[0; 6],
            error_dst_ip,
            orig_ip_header,
            orig_8_bytes,
        ),
        IcmpErrorKind::NetUnreachable => write_icmp_net_unreachable_frame(
            &our_mac,
            our_ip,
            &[0; 6],
            error_dst_ip,
            orig_ip_header,
            orig_8_bytes,
        ),
        IcmpErrorKind::HostUnreachable => write_icmp_host_unreachable_frame(
            &our_mac,
            our_ip,
            &[0; 6],
            error_dst_ip,
            orig_ip_header,
            orig_8_bytes,
        ),
    };

    let mut frame = Frame::new();
    frame.len = arr.len();
    frame.data[..frame.len].copy_from_slice(&arr);

    send_frame_with_arp(stack, frame, next_hop, out_port);
}

// =============================================================================
// Helpers
// =============================================================================

/// Resuelve la ruta para `dst_ip`: devuelve (índice de puerto, IP del next-hop).
/// Si la ruta es directamente conectada (next_hop = None), el next-hop es el propio dst.
fn resolve_route(stack: &Stack, dst_ip: [u8; 4]) -> Option<(usize, [u8; 4])> {
    let entry = stack.routing.lookup(dst_ip)?;
    let next_hop = entry.next_hop.unwrap_or(dst_ip);
    let iface = entry.iface;
    let mut port_idx_opt = None;
    for i in 0..stack.port_count {
        if stack.ports[i].as_ref().unwrap().name == iface {
            port_idx_opt = Some(i);
            break;
        }
    }
    Some((port_idx_opt?, next_hop))
}

fn send_arp_request(stack: &mut Stack, out_port: usize, target_ip: [u8; 4]) {
    let our_mac = stack.ports[out_port].as_ref().unwrap().mac;
    let our_ip = stack.ports[out_port].as_ref().unwrap().ip;
    let mut req = [0u8; ARP_FRAME_SIZE];
    write_arp_request_frame(&mut req, &our_mac, our_ip, target_ip);
    let _ = stack.ports[out_port]
        .as_mut()
        .unwrap()
        .tx
        .send_to(&req, None);
}

/// Detecta paquetes cuya resolución ARP lleva más de ARP_TIMEOUT sin respuesta
/// y envía ICMP Host Unreachable al origen de cada uno.
fn check_arp_timeouts(stack: &mut Stack, ui_tx: &mpsc::Sender<Event>) {
    let now = Instant::now();
    let mut i = 0;
    while i < stack.pending_arp_count {
        let timed_out = match &stack.pending_arp[i] {
            Some((_, pkt)) => now.duration_since(pkt.enqueued_at) >= ARP_TIMEOUT,
            None => false,
        };
        if timed_out {
            let (_, pkt) = stack.pending_arp[i].unwrap();
            // Desplazar restantes hacia la izquierda
            for j in i..stack.pending_arp_count - 1 {
                stack.pending_arp[j] = stack.pending_arp[j + 1];
            }
            stack.pending_arp[stack.pending_arp_count - 1] = None;
            stack.pending_arp_count -= 1;

            // Los errores ICMP no generan otro error ICMP (RFC 792).
            // Detectamos ICMP de error por protocolo=1 y tipo ∈ {3, 11}.
            if pkt.frame.len < ETH_HEADER_SIZE + IP_HEADER_MIN_LEN {
                continue;
            }
            let proto = pkt.frame.data[ETH_HEADER_SIZE + 9];
            let icmp_type = if pkt.frame.len > ETH_HEADER_SIZE + IP_HEADER_MIN_LEN {
                pkt.frame.data[ETH_HEADER_SIZE + IP_HEADER_MIN_LEN]
            } else {
                0
            };
            if proto == 1 && (icmp_type == 3 || icmp_type == 11) {
                continue;
            }

            let eth = EthView {
                data: pkt.frame.as_slice(),
            };
            let ip = Ipv4Header {
                data: eth.payload(),
            };
            let src_ip = ip.src_ip();
            let dst_ip = ip.dst_ip();

            // Si el paquete fue originado por el propio router (ping desde la UI),
            // no enviamos ICMP de error a nosotros mismos; solo logueamos.
            let mut is_ours = false;
            for k in 0..stack.port_count {
                if stack.ports[k].as_ref().unwrap().ip == src_ip {
                    is_ours = true;
                    break;
                }
            }

            if is_ours {
                let _ = ui_tx.send(Event::Log(format!(
                    "Host Unreachable: {} no responde ARP",
                    format_ip(dst_ip)
                )));
                continue;
            }

            let hdr_len = ip.ihl_bytes().min(IP_HEADER_MIN_LEN);
            let mut orig_ip_header = [0u8; IP_HEADER_MIN_LEN];
            orig_ip_header[..hdr_len].copy_from_slice(&eth.payload()[..hdr_len]);

            let mut orig_8_bytes = [0u8; 8];
            let p8 = ip.first_8_payload_bytes();
            let p8_len = p8.len().min(8);
            orig_8_bytes[..p8_len].copy_from_slice(&p8[..p8_len]);

            let _ = ui_tx.send(Event::Log(format!(
                "Host Unreachable: ARP sin respuesta para {}, notificando a {}",
                format_ip(dst_ip),
                format_ip(src_ip)
            )));
            send_icmp_error(
                stack,
                ui_tx,
                src_ip,
                &orig_ip_header,
                &orig_8_bytes,
                IcmpErrorKind::HostUnreachable,
            );
            // No incrementamos i (se desplazó hacia la izquierda)
        } else {
            i += 1;
        }
    }
}

fn deliver_pending_arp(
    stack: &mut Stack,
    ui_tx: &mpsc::Sender<Event>,
    resolved_ip: [u8; 4],
    resolved_mac: [u8; 6],
) {
    let mut i = 0;
    while i < stack.pending_arp_count {
        let matches = match &stack.pending_arp[i] {
            Some((ip, _)) => *ip == resolved_ip,
            None => false,
        };
        if matches {
            let (_, mut pkt) = stack.pending_arp[i].unwrap();
            // Desplazar restantes hacia la izquierda
            for j in i..stack.pending_arp_count - 1 {
                stack.pending_arp[j] = stack.pending_arp[j + 1];
            }
            stack.pending_arp[stack.pending_arp_count - 1] = None;
            stack.pending_arp_count -= 1;

            let out_mac = stack.ports[pkt.out_iface_idx].as_ref().unwrap().mac;
            pkt.frame.data[0..6].copy_from_slice(&resolved_mac);
            pkt.frame.data[6..12].copy_from_slice(&out_mac);
            let _ = stack.ports[pkt.out_iface_idx]
                .as_mut()
                .unwrap()
                .tx
                .send_to(pkt.frame.as_slice(), None);
            let _ = ui_tx.send(Event::Log(format!(
                "ARP resuelto {}, paquete pendiente entregado",
                format_ip(resolved_ip)
            )));
            // No incrementamos i (se desplazó hacia la izquierda)
        } else {
            i += 1;
        }
    }
}

// =============================================================================
// Comandos de la UI
// =============================================================================

fn handle_command(stack: &mut Stack, ui_tx: &mpsc::Sender<Event>, cmd: Command) {
    match cmd {
        Command::Ping(target_ip) => {
            if let Some((out_port, next_hop)) = resolve_route(stack, target_ip) {
                stack.pending_ping = Some(target_ip);
                let our_mac = stack.ports[out_port].as_ref().unwrap().mac;
                let our_ip = stack.ports[out_port].as_ref().unwrap().ip;
                let arr = write_icmp_echo_request_frame(&our_mac, our_ip, &[0; 6], target_ip);
                let mut frame = Frame::new();
                frame.len = ICMP_ECHO_FRAME_LEN;
                frame.data[..ICMP_ECHO_FRAME_LEN].copy_from_slice(&arr);
                send_frame_with_arp(stack, frame, next_hop, out_port);
            } else {
                let _ = ui_tx.send(Event::Log(format!(
                    "Sin ruta para {}",
                    format_ip(target_ip)
                )));
            }
        }
        Command::AddRoute {
            network,
            mask,
            next_hop,
            iface,
        } => {
            let entry = RouteEntry {
                network,
                mask,
                next_hop,
                iface: iface_name(&iface),
            };
            if stack.routing.insert(entry) {
                let _ = ui_tx.send(Event::Log(format!(
                    "Ruta añadida: {}/{}",
                    format_ip(network),
                    u32::from_be_bytes(mask).count_ones()
                )));
            } else {
                let _ = ui_tx.send(Event::Log("Tabla llena".to_string()));
            }
        }
        Command::DelRoute { network, mask } => {
            if stack.routing.remove(network, mask) {
                let _ = ui_tx.send(Event::Log(format!(
                    "Ruta eliminada: {}",
                    format_ip(network)
                )));
            } else {
                let _ = ui_tx.send(Event::Log("Ruta no encontrada".to_string()));
            }
        }
        Command::ShowRoutes => {
            let header = format!(
                "{:<16}{:<16}{:<16}{}",
                "Destination", "Gateway", "Genmask", "Iface"
            );
            let mut s = header;
            let entries = stack.routing.active_entries();
            for k in 0..entries.len() {
                if let Some(e) = &entries[k] {
                    let dst = if e.mask == [0, 0, 0, 0] {
                        "default".to_string()
                    } else {
                        format_ip(e.network)
                    };
                    let gw = match e.next_hop {
                        Some(ip) => format_ip(ip),
                        None => "0.0.0.0".to_string(),
                    };
                    s.push('\n');
                    s.push_str(&format!(
                        "{:<16}{:<16}{:<16}{}",
                        dst,
                        gw,
                        format_ip(e.mask),
                        iface_str(&e.iface)
                    ));
                }
            }
            let _ = ui_tx.send(Event::RouteDump(s));
        }
        Command::ShowArp => {
            let entries = stack.arp_cache.entries();
            let mut s = String::new();
            for k in 0..entries.len() {
                if k > 0 {
                    s.push('\n');
                }
                s.push_str(&format!(
                    "{}  {}",
                    format_ip(entries[k].ip),
                    format_mac(&entries[k].mac)
                ));
            }
            let _ = ui_tx.send(Event::ArpDump(s));
        }
        Command::Quit => unreachable!("Quit se gestiona en el bucle principal"),
    }
}
