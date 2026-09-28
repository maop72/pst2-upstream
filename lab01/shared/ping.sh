#!/bin/sh
# Uso: ping.sh <count> <dst_ip>
#
# Wrapper para ping que evita pasar -c directamente a kathara exec,
# ya que kathara bloquea ese flag por su protección anti-auto-ejecución.
ping -c "$1" -W 2 "$2"
