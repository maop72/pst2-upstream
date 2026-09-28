#!/bin/sh
# Uso: tcpdump.sh <count> <iface> [filtro...]
#
# Wrapper para tcpdump que evita pasar -c directamente a kathara exec.
COUNT="$1"
IFACE="$2"
shift 2
tcpdump -c "$COUNT" -i "$IFACE" "$@"
