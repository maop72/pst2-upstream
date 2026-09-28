#!/bin/sh
# Envía 1 ICMP echo con TTL=1.
# Uso: ttl1_ping.sh <dst_ip>
ping -c 1 -W 2 -t 1 "$1"
