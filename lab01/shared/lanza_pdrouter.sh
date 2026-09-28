#!/bin/sh
ip link set eth0 up;
ip link set eth1 up;
/shared/shutdown_linux.sh;   # evitar interferencia del kernel
/shared/router;

