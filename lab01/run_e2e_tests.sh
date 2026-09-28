#!/bin/sh
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Copiar el lab a un directorio temporal en /tmp para evitar
# problemas en entornos donde Kathará no funciona sobre NFS.
LAB_TMP="$(mktemp -d /tmp/router-lab-XXXXXX)"

cleanup() {
    echo "=== Parando lab Kathará ==="
    echo y | kathara lclean -d "$LAB_TMP" || true
    rm -rf "$LAB_TMP"
}
trap cleanup EXIT

echo "=== Compilando router ==="
cd "$PROJECT_DIR"

# El binario debe ejecutarse en los contenedores Kathará (Debian Bookworm,
# glibc 2.36). Para evitar incompatibilidades cuando el host tenga una
# glibc más nueva, se prefiere la compilación estática con musl.
#
# Estrategia (en orden de preferencia):
#   1. musl-gcc disponible    → cargo build --target musl  (binario estático)
#   2. cargo-zigbuild + zig   → cargo zigbuild --target musl (sin root, sin Docker)
#   3. Docker disponible      → compilar dentro de kathara/base
#   4. Compilación nativa     → puede fallar si glibc del host > 2.36

MUSL_TARGET="x86_64-unknown-linux-musl"

# Si hay un zig descargado en el proyecto, añadirlo al PATH para que
# cargo-zigbuild pueda encontrarlo (útil cuando se ejecuta fuera del CI).
if [ -f "$PROJECT_DIR/.zig/zig" ]; then
    export PATH="$PROJECT_DIR/.zig:$PATH"
fi

if command -v musl-gcc >/dev/null 2>&1 && \
   rustup target list --installed 2>/dev/null | grep -q "$MUSL_TARGET"; then
    echo "  → usando musl-gcc (binario estático)"
    cargo build --release --target "$MUSL_TARGET"
    cp "target/$MUSL_TARGET/release/router" "$SCRIPT_DIR/shared/router"

elif command -v cargo-zigbuild >/dev/null 2>&1 && \
     command -v zig >/dev/null 2>&1 && \
     rustup target list --installed 2>/dev/null | grep -q "$MUSL_TARGET"; then
    echo "  → usando cargo-zigbuild (binario estático)"
    cargo zigbuild --release --target "$MUSL_TARGET"
    cp "target/$MUSL_TARGET/release/router" "$SCRIPT_DIR/shared/router"

elif docker info >/dev/null 2>&1; then
    echo "  → usando docker (kathara/base)"
    docker run --rm \
        -v "$PROJECT_DIR:/project" \
        -v "router-rust-cache:/root/.cargo" \
        kathara/base:latest \
        bash -c "
            set -e
            if ! command -v cargo >/dev/null 2>&1; then
                apt-get update -qq
                apt-get install -y -qq curl build-essential pkg-config
                curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet
            fi
            . /root/.cargo/env
            cd /project
            cargo build --release 2>&1
            cp target/release/router /project/e2e-kathara-lab/shared/router
        "

else
    echo "  → AVISO: compilando en el host (puede fallar si glibc del host > 2.36)"
    cargo build --release
    cp target/release/router "$SCRIPT_DIR/shared/router"
fi

chmod +x "$SCRIPT_DIR/shared/"*.sh "$SCRIPT_DIR/shared/router"

echo "=== Copiando lab a $LAB_TMP ==="
cp -r "$SCRIPT_DIR/." "$LAB_TMP/"

echo "=== Arrancando lab Kathará ==="
echo y | kathara lclean -d "$LAB_TMP"
echo y | kathara lstart -d "$LAB_TMP" --noterminals

echo "=== Ejecutando tests ==="
ROUTER_LAB_DIR="$LAB_TMP" cargo test --test e2e -- --test-threads=1 --nocapture
