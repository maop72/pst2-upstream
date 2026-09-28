#!/bin/sh
# Restaura el procesamiento de paquetes IP y ARP por el kernel.
# Llamar al detener el binario router en espacio de usuario.

# --- iptables ---
if [ -f /tmp/iptables_backup.rules ]; then
    iptables-restore < /tmp/iptables_backup.rules
else
    iptables -P INPUT   ACCEPT
    iptables -P OUTPUT  ACCEPT
    iptables -P FORWARD ACCEPT
fi

# --- arptables ---
if command -v arptables >/dev/null 2>&1; then
    if [ -f /tmp/arptables_backup.rules ]; then
        arptables-restore < /tmp/arptables_backup.rules
    else
        arptables -P INPUT  ACCEPT
        arptables -P OUTPUT ACCEPT
    fi
fi
