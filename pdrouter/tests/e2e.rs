//! Tests end-to-end del router con Kathará.
//!
//! Topología:
//!   hub1 (200.0.0.0/24): pc1=.10, pc4=.40, r1:eth0=.1, r3:eth0=.3
//!   hub2 (201.0.0.0/24): r1:eth1=.1, r2:eth0=.2
//!   hub3 (202.0.0.0/24): r2:eth1=.2, r3:eth1=.3, pc2=.20
//!   hub4 (203.0.0.0/24): r2:eth2=.2, pc3=.30
//!
//! Rutas destacadas:
//!   pc1:  default→r1 (200.0.0.1),  202.0.0.0/24→r3 (200.0.0.3)
//!   pc2:  default→r3 (202.0.0.3),  203.0.0.0/24→r2 (202.0.0.2)
//!   pc4:  default→r3 (200.0.0.3)
//!   r1:   default→r2 (201.0.0.2)
//!   r2:   default→r3 (202.0.0.3)
//!   r3:   default→r2 (202.0.0.2)
//!
//! Prerrequisitos:
//!   1. Compilar el binario y copiarlo a shared/:
//!        cargo build --release && cp target/release/router e2e-kathara-lab/shared/
//!   2. Arrancar el lab:
//!        cd e2e-kathara-lab && kathara lclean; kathara lstart --noterminals
//!
//! Ejecutar:
//!        ROUTER_LAB_DIR=<ruta> cargo test --test e2e -- --test-threads=1 --nocapture
//!
//! O con el script que automatiza todos los pasos:
//!        cd e2e-kathara-lab && ./run_lab.sh

use std::process::Command;
use std::thread;
use std::time::Duration;

// ─── Constantes ───────────────────────────────────────────────────────────────

/// Tiempo de espera inicial para que los routers y los PCs arranquen.
const INIT: Duration = Duration::from_millis(3000);

// ─── Infraestructura ──────────────────────────────────────────────────────────

fn lab_dir() -> String {
    std::env::var("ROUTER_LAB_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/e2e-kathara-lab").to_string())
}

/// Ejecuta un comando dentro de un contenedor Kathará y devuelve su stdout.
fn kexec(host: &str, args: &[&str]) -> String {
    let out = Command::new("kathara")
        .args(["exec", "-d", &lab_dir(), host, "--"])
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("kathara exec {host}: {e}"));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Como kexec pero devuelve stdout + stderr combinados (necesario para mensajes ICMP de error).
fn kexec_combined(host: &str, args: &[&str]) -> String {
    let out = Command::new("kathara")
        .args(["exec", "-d", &lab_dir(), host, "--"])
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("kathara exec {host}: {e}"));
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    )
}

/// Envía `count` pings desde `src` a `dst_ip` y devuelve la salida completa.
fn ping_n(src: &str, dst_ip: &str, count: u32) -> String {
    let count_str = count.to_string();
    kexec(src, &["/shared/ping.sh", &count_str, dst_ip])
}

/// Envía 1 ping y devuelve true si hay respuesta.
fn ping(src: &str, dst_ip: &str) -> bool {
    let out = ping_n(src, dst_ip, 1);
    out.contains("1 received") || out.contains("0% packet loss")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// T01 — Conectividad en la misma red (sin routing)
///
/// Topología relevante:
///   hub1: pc1 (200.0.0.10) ── pc4 (200.0.0.40)
///
/// pc1 envía un ICMP Echo Request a pc4. Como ambos están en el mismo
/// segmento (hub1, 200.0.0.0/24), el paquete no atraviesa ningún router:
/// el kernel de pc1 resuelve la MAC de pc4 vía ARP y entrega el frame
/// directamente en hub1.
///
/// Este test actúa como sanity check de la infraestructura básica:
/// si falla, la causa no está en el router sino en la configuración
/// de red de los PCs (IP, interfaz levantada, ARP).
#[test]
fn t01_misma_red() {
    // Esperamos a que todos los contenedores hayan terminado sus startups.
    thread::sleep(INIT);

    assert!(
        ping("pc1", "200.0.0.40"),
        "FALLO — pc1 (200.0.0.10) no puede hacer ping a pc4 (200.0.0.40).\n\
         \n\
         Este test no usa ningún router: ambos PCs comparten hub1 (200.0.0.0/24).\n\
         El mecanismo es: pc1 envía ARP broadcast → pc4 responde con su MAC\n\
         → pc1 envía ICMP Echo → pc4 responde. Nada más.\n\
         \n\
         Posibles causas:\n\
         - La interfaz eth0 de pc1 o pc4 no está levantada (ip link set eth0 up).\n\
         - La IP no está asignada (ip addr add 200.0.0.x/24 dev eth0).\n\
         - El startup script de algún PC no se ejecutó correctamente.\n\
         \n\
         Si este test falla, todos los demás fallarán también."
    );
}

/// T02 — Routing de un salto vía r3: pc1 → pc2
///
/// Topología relevante:
///   pc1 (200.0.0.10) ──hub1── r3:eth0   r3:eth1 ──hub3── pc2 (202.0.0.20)
///                              200.0.0.3          202.0.0.3
///
/// pc1 tiene una ruta específica: 202.0.0.0/24 via 200.0.0.3 (r3).
/// Aunque su ruta por defecto apunta a r1, el LPM elige r3 para este destino.
///
/// Camino de ida (Echo Request):
///   pc1 → r3 (200.0.0.3) → pc2  [r3 tiene 202.0.0.0/24 directamente conectada]
///
/// Camino de vuelta (Echo Reply):
///   pc2 → r3 (gateway 202.0.0.3) → pc1  [r3 tiene 200.0.0.0/24 directamente conectada]
///
/// Ambos sentidos usan r3, que conecta hub1 con hub3 directamente.
/// r1 y r2 no participan. Este test verifica el forwarding básico de r3
/// y que sus dos interfaces están correctamente configuradas.
#[test]
fn t02_un_salto_via_r3_pc1_pc2() {
    assert!(
        ping("pc1", "202.0.0.20"),
        "FALLO — pc1 (200.0.0.10) no puede hacer ping a pc2 (202.0.0.20).\n\
         \n\
         Camino esperado:\n\
         - Ida:    pc1 → r3 (200.0.0.3) → pc2\n\
         - Vuelta: pc2 → r3 (202.0.0.3) → pc1\n\
         \n\
         Posibles causas de fallo:\n\
         - r3 no tiene la red 200.0.0.0/24 directamente conectada (eth0).\n\
         - r3 no tiene la red 202.0.0.0/24 directamente conectada (eth1).\n\
         - r3 no está respondiendo ARP en hub1 o en hub3.\n\
         - pc2 no tiene configurada una ruta de vuelta a 200.0.0.0/24 (debería usar r3 como default)."
    );
}

/// T03 — Routing de dos saltos: pc1 → pc3
///
/// Topología relevante:
///   pc1 (200.0.0.10) ──hub1── r1:eth0   r1:eth1 ──hub2── r2:eth0   r2:eth2 ──hub4── pc3 (203.0.0.30)
///                              200.0.0.1          201.0.0.1   201.0.0.2          203.0.0.2
///
/// pc1 no tiene ruta específica para 203.0.0.0/24, así que usa su default (r1).
/// r1 tampoco tiene ruta específica para 203.0.0.0/24, así que usa su default (r2).
/// r2 tiene 203.0.0.0/24 directamente conectada (eth2).
///
/// Camino de ida (Echo Request):
///   pc1 → r1 (default 200.0.0.1) → r2 (default 201.0.0.2) → pc3
///
/// Camino de vuelta (Echo Reply):
///   pc3 → r2 (default 203.0.0.2) → r3 (default 202.0.0.3) → pc1
///   [r3 tiene 200.0.0.0/24 directamente conectada, entrega a pc1]
///
/// r3 no participa en la ida pero sí en la vuelta.
#[test]
fn t03_dos_saltos_pc1_pc3() {
    assert!(
        ping("pc1", "203.0.0.30"),
        "FALLO — pc1 (200.0.0.10) no puede hacer ping a pc3 (203.0.0.30).\n\
         \n\
         Camino esperado:\n\
         - Ida:    pc1 → r1 (200.0.0.1) → r2 (201.0.0.2) → pc3\n\
         - Vuelta: pc3 → r2 (203.0.0.2) → r3 (202.0.0.3) → pc1\n\
         \n\
         Posibles causas de fallo:\n\
         - r1 no reenvía a r2 (fallo en su ruta por defecto via 201.0.0.2).\n\
         - r2 no tiene la red 203.0.0.0/24 directamente conectada (eth2).\n\
         - r2 no reenvía de vuelta hacia pc1 (default via r3).\n\
         - r3 no tiene la red 200.0.0.0/24 directamente conectada (eth0).\n\
         - Fallo de ARP en hub2 (r1:eth1 ↔ r2:eth0) o en hub4 (r2:eth2 ↔ pc3)."
    );
}

/// T04 — Routing en sentido inverso: pc3 → pc1
///
/// Topología relevante: igual que T03, sentido contrario.
///
/// Camino de ida (Echo Request):
///   pc3 → r2 (default 203.0.0.2) → r3 (default 202.0.0.3) → pc1
///   [r3 tiene 200.0.0.0/24 directamente conectada]
///
/// Camino de vuelta (Echo Reply):
///   pc1 → r1 (default 200.0.0.1) → r2 (default 201.0.0.2) → pc3
///
/// r1 no participa en la ida pero sí en la vuelta.
/// Verifica especialmente que r2 sabe enrutar hacia 200.0.0.0/24 (vía r3)
/// y que r3 entrega correctamente en hub1.
#[test]
fn t04_vuelta_pc3_pc1() {
    assert!(
        ping("pc3", "200.0.0.10"),
        "FALLO — pc3 (203.0.0.30) no puede hacer ping a pc1 (200.0.0.10).\n\
         \n\
         Camino esperado:\n\
         - Ida:    pc3 → r2 (203.0.0.2) → r3 (202.0.0.3) → pc1\n\
         - Vuelta: pc1 → r1 (200.0.0.1) → r2 (201.0.0.2) → pc3\n\
         \n\
         Posibles causas de fallo:\n\
         - r2 no reenvía hacia r3 (fallo en su ruta por defecto via 202.0.0.3).\n\
         - r3 no tiene la red 200.0.0.0/24 directamente conectada (eth0).\n\
         - r1 no reenvía de vuelta a r2 (default via 201.0.0.2).\n\
         - r2 no tiene la red 203.0.0.0/24 directamente conectada (eth2).\n\
         - Fallo de ARP en hub3 (r2:eth1 ↔ r3:eth1) o en hub1 (r3:eth0 ↔ pc1)."
    );
}

/// T05 — Routing de un salto vía r3: pc4 → pc2
///
/// Topología relevante:
///   pc4 (200.0.0.40) ──hub1── r3:eth0   r3:eth1 ──hub3── pc2 (202.0.0.20)
///                              200.0.0.3          202.0.0.3
///
/// pc4 usa r3 como gateway por defecto (200.0.0.3).
/// r3 tiene 202.0.0.0/24 directamente conectada (eth1), entrega a pc2.
///
/// Camino de ida:   pc4 → r3 → pc2
/// Camino de vuelta: pc2 → r3 → pc4
///
/// Similar a T02 pero iniciado desde pc4, que tiene una tabla de rutas más
/// sencilla (solo default, sin rutas específicas). Verifica que r3 atiende
/// correctamente a dos hosts distintos de hub1.
#[test]
fn t05_un_salto_via_r3_pc4_pc2() {
    assert!(
        ping("pc4", "202.0.0.20"),
        "FALLO — pc4 (200.0.0.40) no puede hacer ping a pc2 (202.0.0.20) vía r3.\n\
         \n\
         Camino esperado:\n\
         - Ida:    pc4 → r3 (200.0.0.3) → pc2\n\
         - Vuelta: pc2 → r3 (202.0.0.3) → pc4\n\
         \n\
         Posibles causas de fallo:\n\
         - r3 no tiene la red 200.0.0.0/24 directamente conectada (eth0).\n\
         - r3 no tiene la red 202.0.0.0/24 directamente conectada (eth1).\n\
         - r3 no responde ARP a pc4 en hub1 (¿conflicto de MAC con r1?).\n\
         - pc4 no tiene configurada la ruta por defecto via 200.0.0.3."
    );
}

/// T06 — Sin pérdida de paquetes en ruta larga: pc1 → pc3
///
/// Topología relevante: igual que T03 (pc1 ──hub1── r1 ──hub2── r2 ──hub4── pc3).
///
/// Envía 10 pings consecutivos de pc1 (200.0.0.10) a pc3 (203.0.0.30) y verifica
/// que ninguno se pierde (0% packet loss). Mientras T03 solo comprueba
/// conectividad básica (al menos un ping llega), este test detecta fallos
/// intermitentes: colisiones en la caché ARP, race conditions en el forwarding,
/// o frames descartados por el filtro MAC.
///
/// Si T03 pasa pero T06 falla, el router procesa correctamente los primeros
/// paquetes pero pierde alguno bajo carga sostenida.
#[test]
fn t06_sin_perdida_pc1_pc3() {
    let out = ping_n("pc1", "203.0.0.30", 10);
    assert!(
        out.contains("0% packet loss"),
        "FALLO — Se pierden paquetes en la ruta pc1 → pc3 (10 pings).\n\
         \n\
         Camino: pc1 → r1 (200.0.0.1) → r2 (201.0.0.2) → pc3\n\
         \n\
         Posibles causas de fallo:\n\
         - Race condition en la caché ARP (entrada expira o se sobreescribe).\n\
         - El router descarta frames bajo carga (buffer del socket lleno).\n\
         - Pérdida intermitente en el forwarding (bug en la lógica de reenvío).\n\
         - Si T03 también falla, la causa es de conectividad, no de fiabilidad.\n\
         \n\
         Salida de ping:\n{out}"
    );
}

/// T07 — ICMP Time Exceeded: ping con TTL=1 desde pc1
///
/// Topología relevante:
///   pc1 (200.0.0.10) ──hub1── r1:eth0 (200.0.0.1)
///
/// pc1 envía un ICMP Echo Request a pc3 (203.0.0.30) con TTL=1.
/// r1 recibe el paquete, decrementa TTL a 0, y en lugar de reenviarlo
/// debe descartarlo y devolver a pc1 un ICMP Time Exceeded (tipo 11, código 0).
///
/// El mensaje de error de ping ("Time to live exceeded") va a stderr,
/// por eso se usa kexec_combined en lugar de kexec.
///
/// Este test verifica que el router implementa correctamente el decremento
/// de TTL y la generación de ICMP Time Exceeded, mecanismo fundamental
/// para que traceroute funcione.
#[test]
fn t07_ttl_exceeded() {
    // ping escribe los mensajes ICMP de error por stderr → kexec_combined
    let out = kexec_combined("pc1", &["/shared/ttl1_ping.sh", "203.0.0.30"]);
    assert!(
        out.to_lowercase().contains("exceeded") || out.to_lowercase().contains("time to live"),
        "FALLO — pc1 no recibió ICMP Time Exceeded de r1.\n\
         \n\
         Se esperaba que r1 decrementara TTL a 0 y devolviera ICMP Time Exceeded\n\
         (tipo 11, código 0) a pc1 (200.0.0.10).\n\
         \n\
         Posibles causas de fallo:\n\
         - r1 no decrementa el TTL antes de reenviar.\n\
         - r1 descarta el paquete cuando TTL=0 pero no genera Time Exceeded.\n\
         - r1 no sabe construir la cabecera ICMP de error o el checksum es incorrecto.\n\
         - r1 no tiene ruta de vuelta a pc1 (200.0.0.0/24 es directamente conectada, eth0).\n\
         \n\
         Salida de ping:\n{out}"
    );
}

/// T08 — ICMP Host Unreachable: ping a host inexistente en red directamente conectada
///
/// Topología relevante:
///   pc1 (200.0.0.10) ──hub1── r1 ──hub2── r2:eth2 ──hub4── [203.0.0.99 no existe]
///
/// pc1 envía un ICMP Echo Request a 203.0.0.99 (dirección sin host en hub4).
/// El paquete llega a r2 (vía r1), que tiene 203.0.0.0/24 directamente conectada (eth2).
/// r2 envía ARP Request para 203.0.0.99 en hub4 y no obtiene respuesta.
/// r2 debe entonces devolver ICMP Host Unreachable (tipo 3, código 1) a pc1.
///
/// El camino de vuelta del ICMP error: r2 → r3 (default) → pc1 (hub1 directo).
///
/// El mensaje de error de ping va a stderr, por eso se usa kexec_combined.
///
/// Este test verifica que el router genera ICMP Host Unreachable cuando
/// el destino está en una red directamente conectada pero el host no responde ARP.
#[test]
fn t08_host_unreachable() {
    // ping escribe los mensajes ICMP de error por stderr → kexec_combined
    let out = kexec_combined("pc1", &["/shared/ping.sh", "1", "203.0.0.99"]);
    assert!(
        out.to_lowercase().contains("unreachable"),
        "FALLO — pc1 no recibió ICMP Host Unreachable de r2.\n\
         \n\
         Se esperaba que r2 hiciera ARP para 203.0.0.99 en hub4, no obtuviera\n\
         respuesta, y devolviera ICMP Host Unreachable (tipo 3, código 1) a pc1.\n\
         \n\
         Posibles causas de fallo:\n\
         - r2 no genera Host Unreachable cuando ARP no obtiene respuesta.\n\
         - r2 descarta el paquete silenciosamente sin generar el ICMP de error.\n\
         - r2 no sabe construir ICMP Host Unreachable o el checksum es incorrecto.\n\
         - El ICMP de error no llega a pc1 (r2→r3→pc1: fallo en ruta de vuelta).\n\
         - r1 no reenvía el paquete original a r2 (fallo en default via 201.0.0.2).\n\
         \n\
         Salida de ping:\n{out}"
    );
}

/// T09 — Tráfico simultáneo sin pérdidas: pc1 ↔ pc3
///
/// Topología relevante: igual que T03/T04 (r1, r2, r3 en el camino).
///
/// Lanza dos flujos de ping en paralelo desde hilos distintos:
///   - pc1 (200.0.0.10) → pc3 (203.0.0.30): 10 pings
///   - pc3 (203.0.0.30) → pc1 (200.0.0.10): 10 pings
///
/// Ambos flujos cruzan r1, r2 y r3 simultáneamente.
/// Verifica que el router maneja correctamente la concurrencia: no mezcla
/// los flujos, no descarta paquetes bajo carga y su estado interno
/// (caché ARP, tabla de rutas) es correcto bajo acceso simultáneo.
///
/// Si T03 y T04 pasan pero T09 falla, el problema es de concurrencia
/// en el router, no de conectividad.
#[test]
fn t09_trafico_simultaneo() {
    let h1 = thread::spawn(|| ping_n("pc1", "203.0.0.30", 10)); // pc1 → pc3
    let h2 = thread::spawn(|| ping_n("pc3", "200.0.0.10", 10)); // pc3 → pc1

    let out1 = h1.join().expect("hilo pc1 panicked");
    let out2 = h2.join().expect("hilo pc3 panicked");

    assert!(
        out1.contains("0% packet loss"),
        "FALLO — Pérdida de paquetes en pc1 → pc3 bajo tráfico simultáneo.\n\
         \n\
         Posibles causas de fallo:\n\
         - Race condition en la caché ARP al acceder desde múltiples hilos.\n\
         - El router mezcla paquetes de ambos flujos (bug en el forwarding).\n\
         - Buffer del socket saturado bajo carga cruzada.\n\
         - Si T03 también falla, la causa es de conectividad, no de concurrencia.\n\
         \n\
         Salida pc1 → pc3:\n{out1}"
    );
    assert!(
        out2.contains("0% packet loss"),
        "FALLO — Pérdida de paquetes en pc3 → pc1 bajo tráfico simultáneo.\n\
         \n\
         Posibles causas de fallo:\n\
         - Race condition en la caché ARP al acceder desde múltiples hilos.\n\
         - El router mezcla paquetes de ambos flujos (bug en el forwarding).\n\
         - Buffer del socket saturado bajo carga cruzada.\n\
         - Si T04 también falla, la causa es de conectividad, no de concurrencia.\n\
         \n\
         Salida pc3 → pc1:\n{out2}"
    );
}

/// T10 — Longest Prefix Match: la ruta específica gana sobre la ruta general
///
/// Topología relevante:
///   pc3 (203.0.0.30) ──hub4── r2:eth2   r2:eth1 ──hub3── pc2 (202.0.0.20)
///                              203.0.0.2          202.0.0.2
///
/// r2 tiene dos rutas que coinciden con 202.0.0.20:
///   - 202.0.0.0/24  directamente conectada (eth1)  ← específica, entrega directa
///   - 202.0.0.0/16  via r1 (201.0.0.1, eth0)       ← general, 1 salto extra
///
/// Con LPM correcto, r2 elige /24 y entrega directamente a pc2 en hub3.
/// Sin LPM (elige /16), r2 reenvía a r1, que decrementa TTL y lo devuelve
/// a r2, que finalmente entrega — pero con un salto de más.
///
/// El test envía el ping con TTL=2 desde pc3:
///   - Con LPM (/24 directo): pc3→r2→pc2, r2 decrementa TTL a 1 → llega a pc2 ✓
///   - Sin LPM (/16 via r1):  pc3→r2→r1, r1 decrementa TTL a 0 → Time Exceeded ✗
#[test]
fn t10_lpm_ruta_especifica() {
    // Los mensajes ICMP de error (Time Exceeded) van a stderr → kexec_combined
    let out = kexec_combined("pc3", &["/shared/ttl2_ping.sh", "202.0.0.20"]);
    assert!(
        out.contains("1 received") || out.contains("0% packet loss"),
        "FALLO — pc3 no recibió respuesta de pc2 (202.0.0.20) con TTL=2.\n\
         \n\
         Esto indica que r2 eligió la ruta 202.0.0.0/16 via r1 en lugar de\n\
         202.0.0.0/24 directamente conectada:\n\
         con TTL=2 y 2 saltos de router (r2→r1), el TTL llega a 0 en r1 y\n\
         se genera Time Exceeded en lugar de llegar a pc2.\n\
         \n\
         r2 tiene dos rutas para 202.0.0.20:\n\
         - Específica: 202.0.0.0/24 directa (eth1)       — 1 salto, TTL=2 llega a pc2 ✓\n\
         - General:    202.0.0.0/16 via r1 (201.0.0.1)   — 2 saltos, TTL=2 se agota ✗\n\
         \n\
         Posibles causas de fallo:\n\
         - r2 no implementa LPM y usa la primera ruta que encuentra.\n\
         - r2 recorre las rutas en orden de inserción en lugar de por longitud de prefijo.\n\
         - El parser de /etc/network/interfaces insertó las rutas en orden incorrecto.\n\
         \n\
         Salida de ping:\n{out}"
    );
}

/// T11 — ICMP Net Unreachable: ping a red sin ruta en r1
///
/// Topología relevante:
///   pc1 (200.0.0.10) ──hub1── r1 (200.0.0.1 / 201.0.0.1)
///
/// r1 tiene rutas explícitas para 200.0.0.0/24, 201.0.0.0/24, 202.0.0.0/24 y
/// 203.0.0.0/24, pero NO tiene ruta por defecto. Un datagrama con destino
/// 10.0.0.1 no coincide con ninguna entrada de la tabla de r1, que debe
/// generar ICMP Net Unreachable (tipo 3, código 0) directamente a pc1.
///
/// A diferencia de T08 (Host Unreachable), aquí el error se genera antes de
/// intentar ARP: r1 descarta el paquete en la fase de lookup de ruta, sin
/// siquiera saber si el host existe.
///
/// El mensaje de error de ping va a stderr, por eso se usa kexec_combined.
#[test]
fn t11_net_unreachable() {
    // ping escribe los mensajes ICMP de error por stderr → kexec_combined
    let out = kexec_combined("pc1", &["/shared/ping.sh", "1", "10.0.0.1"]);
    assert!(
        out.to_lowercase().contains("unreachable"),
        "FALLO — pc1 no recibió ICMP Net Unreachable de r1.\n\
         \n\
         Se esperaba que r1 buscara 10.0.0.1 en su tabla de rutas, no\n\
         encontrara ninguna entrada coincidente (r1 no tiene ruta por defecto)\n\
         y devolviera ICMP Net Unreachable (tipo 3, código 0) a pc1.\n\
         \n\
         Posibles causas de fallo:\n\
         - r1 tiene una ruta por defecto que no debería tener (revisar\n\
           e2e-kathara-lab/r1/etc/network/interfaces).\n\
         - r1 descarta el paquete silenciosamente sin generar el ICMP de error.\n\
         - r1 no construye correctamente ICMP Net Unreachable (tipo 3, código 0).\n\
         - El ICMP de error no llega a pc1 (r1 tiene 200.0.0.0/24 directa,\n\
           debería llegar sin problema).\n\
         \n\
         Salida de ping:\n{out}"
    );
}
