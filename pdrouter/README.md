# PDRouter: Programmable Didactic Router

Router en espacio de usuario (Rust + pnet)

Router IPv4 implementado completamente en espacio de usuario mediante raw sockets
(`AF_PACKET`). Gestiona ARP, forwarding IP, ICMP Echo Reply, Time Exceeded y
Host Unreachable sin depender del stack de red del kernel.

---

## Arquitectura

```
┌─────────────────────────────────────────────┐
│  Hilo UI  (ui.rs / tui.rs)                  │
│  – Comandos interactivos (ver abajo)        │
│  – Modo ratatui si hay TTY, plain si no     │
└────────────────┬────────────────────────────┘
                 │  mpsc (Command / Event)
┌────────────────▼────────────────────────────┐
│  Hilo de red  (stack.rs)                    │
│  – Poll de interfaces con timeout 1 ms      │
│  – ARP request/reply + caché (TTL 30 s)     │
│  – Forwarding IP + decremento TTL           │
│  – ICMP Echo Reply / Time Exceeded /        │
│    Host Unreachable                         │
└─────────────────────────────────────────────┘
```

La configuración de interfaces y rutas se lee de `/etc/network/interfaces`
(formato Debian) al arrancar. **El binario gestiona todo: el kernel no tiene
IPs configuradas en los routers.**

---

## Comandos disponibles

| Comando                                        | Descripción                        |
|------------------------------------------------|------------------------------------|
| `route`                                        | Muestra la tabla de rutas          |
| `arp`                                          | Muestra la caché ARP               |
| `ping <ip>`                                    | Envía un ICMP Echo Request         |
| `add route <red>/<prefijo> [via <gw>] dev <iface>` | Añade una ruta estática        |
| `del route <red>/<prefijo>`                    | Elimina una ruta                   |
| `help`                                         | Muestra la ayuda                   |
| `quit` / `exit`                                | Apaga el router                    |

En modo ratatui (TTY), las flechas ↑/↓ navegan el historial de comandos.

---

## Escenario de prueba (Kathará)

```
hub1 ── pc1 (200.0.0.10)
     ── pc4 (200.0.0.40)
     ── r1:eth0 (200.0.0.1)
     ── r3:eth0 (200.0.0.3)

hub2 ── r1:eth1 (201.0.0.1)
     ── r2:eth0 (201.0.0.2)

hub3 ── r2:eth1 (202.0.0.2)
     ── r3:eth1 (202.0.0.3)
     ── pc2 (202.0.0.20)

hub4 ── r2:eth2 (203.0.0.2)
     ── pc3 (203.0.0.30)
```

Rutas estáticas configuradas en `/etc/network/interfaces` de cada router:

| Router | Red destino    | Via         | Interfaz |
|--------|----------------|-------------|----------|
| r1     | 202.0.0.0/24   | 201.0.0.2   | eth1     |
| r1     | 203.0.0.0/24   | 201.0.0.2   | eth1     |
| r2     | 200.0.0.0/24   | 201.0.0.1   | eth0     |
| r2     | 202.0.0.0/16   | 201.0.0.1   | eth0     |
| r2     | default        | 202.0.0.3   | eth1     |
| r3     | 201.0.0.0/24   | 200.0.0.1   | eth0     |
| r3     | 203.0.0.0/24   | 202.0.0.2   | eth1     |
| r3     | default        | 202.0.0.2   | eth1     |

---

## Compilar

```sh
cargo build --release
# Binario en target/release/router
```

---

## Ejecutar manualmente (requiere root / CAP_NET_RAW)

```sh
ip link set eth0 up;
ip link set eth1 up;
/shared/shutdown_linux.sh;   # evitar interferencia del kernel
sudo /shared/router;
```

---

## Desactivar / reactivar el stack de red del kernel

Cuando el binario router gestiona el tráfico directamente, el kernel no debe
interferir respondiendo a ARP ni procesando paquetes IP.

### `shared/shutdown_linux.sh` — desactivar

Guarda las reglas actuales y bloquea el kernel:

```sh
/shared/shutdown_linux.sh
```

- Hace backup de iptables en `/tmp/iptables_backup.rules`
- Establece política **DROP** en INPUT, OUTPUT y FORWARD
- Vacía todas las cadenas iptables (incluidas nat y mangle)
- Establece política **DROP** en arptables INPUT y OUTPUT (si está disponible)

### `shared/boot_linux.sh` — reactivar

Restaura el estado original:

```sh
/shared/boot_linux.sh
```

- Restaura iptables desde el backup (o deja política ACCEPT si no hay backup)
- Restaura arptables desde el backup (o deja política ACCEPT si no hay backup)

> Los nodos PC del escenario usan el stack del kernel (`ip addr` + `ip route`)
> y **no** deben ejecutar `shutdown_linux.sh`.

---

## Tests

### Tests unitarios (45 tests)

Cubren los módulos `arp`, `routing`, `interfaces`, `icmp`, `ipv4`, `utils` y `eth`.
No requieren red ni permisos especiales:

```sh
cargo test --bins
```

### Tests e2e con Kathará

El script `e2e-kathara-lab/run_lab.sh` automatiza todo: compila, despliega el
lab y ejecuta los 10 tests en serie.

```sh
e2e-kathara-lab/run_lab.sh
```

Los startups de los routers (`r1.startup`, `r2.startup`, `r3.startup`) llaman
automáticamente a `shutdown_linux.sh` antes de lanzar el binario.

Para filtrar un test concreto:

```sh
ROUTER_LAB_DIR=<lab_dir> cargo test --test e2e t03 -- --test-threads=1 --nocapture
```

### Tests

| Test | Descripción |
|------|-------------|
| T01  | Ping entre pc1 y pc4 (misma red, sin router) |
| T02  | pc1 → pc2 vía r3 (ruta específica, 1 salto) |
| T03  | pc1 → pc3 vía r1 → r2 (2 saltos) |
| T04  | pc3 → pc1 (vuelta, 2 saltos) |
| T05  | pc4 → pc2 vía r3 |
| T06  | 10 pings pc1 → pc3 sin pérdidas |
| T07  | TTL=1 desde pc1 → r1 debe devolver ICMP Time Exceeded |
| T08  | pc1 → 203.0.0.99 (host inexistente) → r2 devuelve ICMP Host Unreachable |
| T09  | Tráfico simultáneo pc1 ↔ pc3 sin pérdidas |
| T10  | LPM: pc3 → pc2 con TTL=2 (ruta directa 202.0.0.0/24, no la /16) |
| T11  | pc1 → 10.0.0.1 (red desconocida) → r1 devuelve ICMP Net Unreachable |

> r1 no tiene ruta por defecto: conoce explícitamente 200.0.0.0/24, 201.0.0.0/24,
> 202.0.0.0/24 y 203.0.0.0/24, pero cualquier otra red genera Net Unreachable.

---

## Estructura del código

```
src/
  main.rs        – Punto de entrada: lee interfaces, arranca hilos UI y stack
  stack.rs       – Hilo de red: ARP, forwarding, ICMP, timeouts
  ui.rs          – Bucle de comandos de usuario
  tui.rs         – TUI (ratatui si TTY, plain si no)
  interfaces.rs  – Parser de /etc/network/interfaces
  routing.rs     – Tabla de rutas con Longest Prefix Match
  arp.rs         – Vista y construcción de frames ARP + caché con expiración
  eth.rs         – Vista y construcción de cabeceras Ethernet
  ipv4.rs        – Vista y modificación de cabeceras IPv4
  icmp.rs        – Vista y construcción de mensajes ICMP
  utils.rs       – Checksum RFC 1071, parseo IP/máscara
```
