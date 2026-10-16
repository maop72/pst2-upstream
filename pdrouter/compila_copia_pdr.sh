#!/bin/bash

# deploy.sh

# Compila pdrotuer con musl y lo copia en el directorio shared del
# lab kathará

# musl: implementación ligera de la biblioteca estándar de C para
# Linux, alternativa a glibc. Orientada a generar ejecutables pequeños 
# y binarios enlazados estáticamente.  

# Para poder compilar en el host:
# sudo apt install musl-tools
# rustup target add x86_64-unknown-linux-musl



LAB=~/pst2/lab01
PDROUTER=~/pst2/pdrouter

#cargo build --target x86_64-unknown-linux-musl
$PDROUTER/compila_contenedor.sh
cp $PDROUTER/target/x86_64-unknown-linux-musl/debug/router  $LAB/shared

