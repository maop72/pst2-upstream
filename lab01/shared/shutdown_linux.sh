#!/bin/sh
# Desactiva el procesamiento de paquetes IP y ARP por el kernel.
# Llamar antes de lanzar el binario router en espacio de usuario.

# --- iptables ---
iptables-save > /tmp/iptables_backup.rules 2>/dev/null

iptables -P INPUT   DROP
iptables -P OUTPUT  DROP
iptables -P FORWARD DROP
iptables -F
iptables -t nat    -F 2>/dev/null
iptables -t mangle -F 2>/dev/null

# --- arptables ---
if command -v arptables >/dev/null 2>&1; then
    arptables-save > /tmp/arptables_backup.rules 2>/dev/null
    arptables -P INPUT  DROP
    arptables -P OUTPUT DROP
    arptables -F
fi
