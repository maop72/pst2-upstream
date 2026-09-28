#!/bin/sh
# Envía 1 ICMP echo con TTL=2.
# Uso: ttl2_ping.sh <dst_ip>
ping -c 1 -W 2 -t 2 "$1"
