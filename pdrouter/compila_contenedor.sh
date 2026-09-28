#!/bin/bash
# compila_contenedor.sh
# Miguel Ortuño, Sept 2026

# Compila PDRouter utilizando un contenedor Docker preparado para generar
# ejecutables estáticos para x86_64 mediante musl. Necesitamos esta
# compilación estática porque los contenedores donde se ejecutará pdrouter
# no disponen de glibc.
#
# mortuno/rustc_musl es una imagen con Ubuntu, Rust y las herramientas
# necesarias (incluido musl) ya preparadas para realizar esta compilación.
# El proyecto se copia al contenedor, se compila allí y el ejecutable
# resultante se copia de vuelta al directorio target del proyecto en el host.
# El proyecto del host nunca se monta en el contenedor.
#
# BUILD_TYPE permite elegir entre una compilación debug o release.


CONTAINER="rustc_musl01"
IMAGE="mortuno/rustc_musl"
PROJECT="$HOME/pst2/pdrouter"
CONTAINER_PROJECT="/home/usuario/pdrouter"
TARGET="x86_64-unknown-linux-musl"

# Tipo de compilación: debug o release
BUILD_TYPE="debug"

# Crear siempre un contenedor nuevo
if docker inspect "$CONTAINER" >/dev/null 2>&1; then
    docker rm -f "$CONTAINER" >/dev/null
fi

echo "Creando contenedor..."
docker create \
    --name "$CONTAINER" \
    "$IMAGE" \
    sleep infinity >/dev/null

docker start "$CONTAINER" >/dev/null

# Eliminar la copia anterior del proyecto
docker exec "$CONTAINER" \
    rm -rf "$CONTAINER_PROJECT"

# Copiar el proyecto al contenedor
docker cp "$PROJECT" "$CONTAINER":/home/usuario/

# Cambiar el propietario al usuario del contenedor
docker exec "$CONTAINER" \
    chown -R usuario:usuario "$CONTAINER_PROJECT"

# Preparar las opciones de cargo y la ruta del ejecutable
if [ "$BUILD_TYPE" = "release" ]; then
    PROFILE="--release"
    BINARY_DIR="release"
else
    PROFILE=""
    BINARY_DIR="debug"
fi

BINARY="$CONTAINER_PROJECT/target/$TARGET/$BINARY_DIR/router"
HOST_BINARY="$PROJECT/target/$TARGET/$BINARY_DIR/router"

# Compilar
docker exec -u usuario \
    -w "$CONTAINER_PROJECT" \
    "$CONTAINER" \
    cargo build $PROFILE --target "$TARGET"

# Comprobar que la compilación ha producido el ejecutable
if ! docker exec "$CONTAINER" test -f "$BINARY"; then
    echo "Error: no se ha generado el ejecutable"
    exit 1
fi

# Crear el directorio de destino en el host
mkdir -p "$(dirname "$HOST_BINARY")"

# Copiar el ejecutable al mismo lugar que usaría Cargo
docker cp "$CONTAINER:$BINARY" "$HOST_BINARY"

echo "Compilación $BUILD_TYPE completada: $HOST_BINARY"
