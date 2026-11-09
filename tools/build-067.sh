#!/usr/bin/env bash
set -euo pipefail

APP_VERSION="0.6.7"
APP_VERSION_CODE="67"
ABI="arm64-v8a"
RUST_TARGET="aarch64-linux-android"
ANDROID_PLATFORM="29"
PACKAGE="com.klisee.reloaded"
MIN_SDK="26"
TARGET_SDK="36"
BUILD_TOOLS_VERSION="37.0.0"

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_DIR="$ROOT/android"
ENGINE_DIR="$ROOT/src/OpenGMK"
OUT_DIR="$ROOT/dist/$APP_VERSION"
INSTALL=1
LAUNCH=0
RUN_PROTOCOL_TESTS=1
ALLOW_DIRTY=0
KEEP_WORK=0
SIGNING_PROPS="${KEYSTORE_PROPERTIES:-}"

usage() {
    printf '%s\n' \
        "Uso: tools/build-067.sh [opciones]" \
        "" \
        "Compila el motor aarch64, comprueba el protocolo 0.6.3 de forma offline," \
        "empaqueta el APK 0.6.7 (versionCode 67), lo firma, calcula su SHA-256 y" \
        "verifica el resultado. Por defecto también lo instala; no lo lanza." \
        "" \
        "Opciones:" \
        "  --no-install                  No instalar en el dispositivo." \
        "  --launch                      Lanzar la app tras instalar (smoke test; la app" \
        "                                puede usar su red normal para el blog)." \
        "  --skip-protocol-tests         No ejecutar los tests locales de protocolo." \
        "  --out DIR                     Directorio de salida (por defecto dist/0.6.7)." \
        "  --keystore-properties F       Fichero con storeFile/storePassword/keyAlias/keyPassword." \
        "  --allow-dirty                 Permite construir con cambios sin commitear." \
        "  --keep-work                   Conserva los ficheros temporales de la firma." \
        "  -h, --help                    Muestra esta ayuda." \
        "" \
        "Variables: ANDROID_SDK_ROOT, ANDROID_NDK_HOME, CARGO_TARGET_DIR," \
        "KEYSTORE_PROPERTIES, RELOADED_KEYSTORE_FILE, RELOADED_KEYSTORE_PASSWORD," \
        "RELOADED_KEY_ALIAS, RELOADED_KEY_PASSWORD, EXPECTED_CERT_SHA256."
}

log() {
    printf '==> %s\n' "$*"
}

warn() {
    printf 'aviso: %s\n' "$*" >&2
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "falta '$1' en PATH"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --no-install)
            INSTALL=0
            ;;
        --launch)
            LAUNCH=1
            ;;
        --skip-protocol-tests)
            RUN_PROTOCOL_TESTS=0
            ;;
        --out)
            [ "$#" -ge 2 ] || die "--out necesita un directorio"
            OUT_DIR="$2"
            shift
            ;;
        --keystore-properties)
            [ "$#" -ge 2 ] || die "--keystore-properties necesita un fichero"
            SIGNING_PROPS="$2"
            shift
            ;;
        --allow-dirty)
            ALLOW_DIRTY=1
            ;;
        --keep-work)
            KEEP_WORK=1
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            die "opcion desconocida: $1"
            ;;
    esac
    shift
done

if [ "$LAUNCH" -eq 1 ] && [ "$INSTALL" -ne 1 ]; then
    die "--launch necesita instalar; quita --no-install"
fi

export LC_ALL=C
export TZ=UTC
umask 022

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/reloaded-067.XXXXXX")"
cleanup() {
    if [ "$KEEP_WORK" -eq 1 ]; then
        printf 'temporales: %s\n' "$WORK_DIR" >&2
    else
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

find_sdk() {
    local candidate
    for candidate in "${ANDROID_SDK_ROOT:-}" "${ANDROID_HOME:-}" \
        "$HOME/Android/Sdk" "$HOME/android-sdk" /opt/android-sdk; do
        if [ -n "$candidate" ] && [ -d "$candidate/platform-tools" ] && [ -d "$candidate/build-tools" ]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

SDK="$(find_sdk)" || die "no encuentro el SDK de Android; define ANDROID_SDK_ROOT"
export ANDROID_SDK_ROOT="$SDK"
export ANDROID_HOME="$SDK"

if [ -n "${BUILD_TOOLS:-}" ]; then
    BUILD_TOOLS_DIR="$BUILD_TOOLS"
elif [ -d "$SDK/build-tools/$BUILD_TOOLS_VERSION" ]; then
    BUILD_TOOLS_DIR="$SDK/build-tools/$BUILD_TOOLS_VERSION"
else
    BUILD_TOOLS_DIR="$(find "$SDK/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)"
fi
AAPT2="$BUILD_TOOLS_DIR/aapt2"
APKSIGNER="$BUILD_TOOLS_DIR/apksigner"
ZIPALIGN="$BUILD_TOOLS_DIR/zipalign"

if [ -n "${ANDROID_NDK_HOME:-}" ]; then
    NDK="$ANDROID_NDK_HOME"
elif [ -n "${ANDROID_NDK_ROOT:-}" ]; then
    NDK="$ANDROID_NDK_ROOT"
elif [ -n "${NDK_VERSION:-}" ] && [ -d "$SDK/ndk/$NDK_VERSION" ]; then
    NDK="$SDK/ndk/$NDK_VERSION"
else
    NDK="$(find "$SDK/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)"
fi
[ -d "$NDK" ] || die "no encuentro el NDK de Android; define ANDROID_NDK_HOME"
export ANDROID_NDK_HOME="$NDK"
export ANDROID_NDK_ROOT="$NDK"

ADB="${ADB:-$SDK/platform-tools/adb}"

need_cmd git
need_cmd cargo
need_cmd rustup
need_cmd sha256sum
need_cmd unzip
need_cmd strings
need_cmd sed
need_cmd awk
need_cmd od
need_cmd tee
[ -x "$AAPT2" ] || die "falta aapt2 en $BUILD_TOOLS_DIR"
[ -x "$APKSIGNER" ] || die "falta apksigner en $BUILD_TOOLS_DIR"
[ -x "$ZIPALIGN" ] || die "falta zipalign en $BUILD_TOOLS_DIR"
cargo ndk --version >/dev/null 2>&1 || die "falta cargo-ndk; instala cargo-ndk y vuelve a intentarlo"

if ! rustup target list --installed | grep -qx "$RUST_TARGET"; then
    die "falta el target $RUST_TARGET; ejecuta: rustup target add $RUST_TARGET"
fi

if [ "$ALLOW_DIRTY" -ne 1 ]; then
    [ -z "$(git -C "$ROOT" status --porcelain)" ] ||
        die "el arbol tiene cambios sin commitear; usa --allow-dirty solo para pruebas"
fi

if grep -R -n -E 'global\.mods_available|file_exists\("mods/|text_op5|draw_text_color\(160|var y0,h,step|mouse_x>=80 && mouse_x<=240' \
    --include='*.rs' "$ENGINE_DIR/gm8emulator/src" >/dev/null; then
    die "el motor contiene GML derivado; esta verificación no admite inyección GML"
fi

if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    warn "CARGO_TARGET_DIR está definido; se usará también para empaquetar el release"
fi

GIT_FULL="$(git -C "$ROOT" rev-parse HEAD)"
GIT_SHORT="$(git -C "$ROOT" rev-parse --short=12 HEAD)"
GIT_BRANCH="$(git -C "$ROOT" branch --show-current)"
SOURCE_DATE_EPOCH="$(git -C "$ROOT" log -1 --format=%ct)"
export SOURCE_DATE_EPOCH

if [ -z "$SIGNING_PROPS" ]; then
    if [ -f "$ANDROID_DIR/keystore.properties" ]; then
        SIGNING_PROPS="$ANDROID_DIR/keystore.properties"
    fi
fi

if [ -n "${RELOADED_KEYSTORE_FILE:-}" ]; then
    KEYSTORE_FILE="$RELOADED_KEYSTORE_FILE"
    KEYSTORE_PASSWORD="${RELOADED_KEYSTORE_PASSWORD:-}"
    KEY_ALIAS="${RELOADED_KEY_ALIAS:-}"
    KEY_PASSWORD="${RELOADED_KEY_PASSWORD:-$KEYSTORE_PASSWORD}"
else
    [ -n "$SIGNING_PROPS" ] || die "falta la configuracion de firma; usa --keystore-properties o RELOADED_KEYSTORE_FILE"
    [ -f "$SIGNING_PROPS" ] || die "no existe el fichero de firma: $SIGNING_PROPS"
    KEYSTORE_FILE=""
    KEYSTORE_PASSWORD=""
    KEY_ALIAS=""
    KEY_PASSWORD=""
    while IFS='=' read -r key value || [ -n "$key" ]; do
        key="${key%$'\r'}"
        value="${value%$'\r'}"
        key="${key#"${key%%[![:space:]]*}"}"
        key="${key%"${key##*[![:space:]]}"}"
        value="${value#"${value%%[![:space:]]*}"}"
        value="${value%"${value##*[![:space:]]}"}"
        case "$key" in
            storeFile) KEYSTORE_FILE="$value" ;;
            storePassword) KEYSTORE_PASSWORD="$value" ;;
            keyAlias) KEY_ALIAS="$value" ;;
            keyPassword) KEY_PASSWORD="$value" ;;
        esac
    done < "$SIGNING_PROPS"
fi

[ -n "$KEYSTORE_FILE" ] || die "la configuracion de firma no define storeFile"
[ -n "$KEYSTORE_PASSWORD" ] || die "la configuracion de firma no define storePassword"
[ -n "$KEY_ALIAS" ] || die "la configuracion de firma no define keyAlias"
[ -n "$KEY_PASSWORD" ] || KEY_PASSWORD="$KEYSTORE_PASSWORD"
case "$KEYSTORE_FILE" in
    /*) ;;
    *)
        if [ -f "$ANDROID_DIR/$KEYSTORE_FILE" ]; then
            KEYSTORE_FILE="$ANDROID_DIR/$KEYSTORE_FILE"
        elif [ -n "$SIGNING_PROPS" ] && [ -f "$(dirname "$SIGNING_PROPS")/$KEYSTORE_FILE" ]; then
            KEYSTORE_FILE="$(dirname "$SIGNING_PROPS")/$KEYSTORE_FILE"
        else
            KEYSTORE_FILE="$ANDROID_DIR/$KEYSTORE_FILE"
        fi
        ;;
esac
[ -f "$KEYSTORE_FILE" ] || die "no existe el keystore: $KEYSTORE_FILE"

export RELOADED_KEYSTORE_PASSWORD="$KEYSTORE_PASSWORD"
export RELOADED_KEY_PASSWORD="$KEY_PASSWORD"

TARGET_DIR="${CARGO_TARGET_DIR:-$ENGINE_DIR/target}"
SO_PATH="$TARGET_DIR/$RUST_TARGET/release/libgm8emulator.so"
UNSIGNED_APK="$ANDROID_DIR/app/build/outputs/apk/release/pokemon-reloaded-$APP_VERSION-$ABI.apk"
ALIGNED_APK="$WORK_DIR/pokemon-reloaded-$APP_VERSION-$ABI-aligned.apk"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd -- "$OUT_DIR" && pwd)"
SIGNED_APK="$OUT_DIR/pokemon-reloaded-$APP_VERSION-$ABI.apk"

log "$APP_VERSION ($APP_VERSION_CODE) $ABI desde $GIT_SHORT${GIT_BRANCH:+ ($GIT_BRANCH)}"
log "SDK $SDK"
log "NDK $NDK"
log "build-tools $BUILD_TOOLS_DIR"

PROTOCOL_TEST_STATUS="omitidos"
PROTOCOL_PASSED=0
PROTOCOL_FILTERS=0
PROTOCOL_DETAIL=""

run_protocol_test() {
    local name="$1" package="$2" filter="$3" minimum="$4"
    local log_file="$WORK_DIR/protocol-$name.txt"
    log "prueba offline $name: cargo test -p $package --lib $filter"
    (
        cd "$ENGINE_DIR"
        cargo test --locked -p "$package" --lib "$filter"
    ) 2>&1 | tee "$log_file"
    local passed
    passed="$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed; 0 failed.*/\1/p' "$log_file" | tail -n 1)"
    [ -n "$passed" ] || die "la prueba $name no terminó en ok/0 failed"
    [ "$passed" -ge "$minimum" ] || die "la prueba $name ejecutó $passed tests; se esperaban al menos $minimum"
    PROTOCOL_PASSED=$((PROTOCOL_PASSED + passed))
    PROTOCOL_FILTERS=$((PROTOCOL_FILTERS + 1))
    PROTOCOL_DETAIL="${PROTOCOL_DETAIL}${PROTOCOL_DETAIL:+, }$name=$passed"
}

if [ "$RUN_PROTOCOL_TESTS" -eq 1 ]; then
    log "verificando el protocolo 0.6.3 con transports mock locales (sin red)"
    run_protocol_test "handshake" "reloaded-cleanroom" "handshake" 1
    run_protocol_test "welcome_codec" "reloaded-cleanroom" "welcome_and_ok" 1
    run_protocol_test "welcome_host" "gm8emulator" "welcome" 3
    run_protocol_test "wok_host" "gm8emulator" "wok" 2
    run_protocol_test "peer_left_host" "gm8emulator" "peer_left" 4
    PROTOCOL_TEST_STATUS="ok ($PROTOCOL_PASSED passed en $PROTOCOL_FILTERS filtros)"
else
    warn "tests de protocolo omitidos; no cuenta como verificación completa"
fi

log "compilando el motor (${CARGO_TARGET_DIR:+target compartido: }${CARGO_TARGET_DIR:-$TARGET_DIR})"
(
    cd "$ENGINE_DIR"
    cargo ndk -t "$ABI" --platform "$ANDROID_PLATFORM" build -p gm8emulator --release --locked
)

[ -s "$SO_PATH" ] || die "el motor no genero $SO_PATH"
magic="$(od -An -tx1 -N4 "$SO_PATH" | tr -d ' \n')"
[ "$magic" = "7f454c46" ] || die "$SO_PATH no es un ELF valido"
SO_SHA="$(sha256sum "$SO_PATH" | awk '{print $1}')"

READELF="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf"
if [ -x "$READELF" ]; then
    machine="$("$READELF" -h "$SO_PATH" | awk -F: '/Machine:/{gsub(/^ +/, "", $2); print $2}')"
    case "$machine" in
        AArch64*) ;;
        *) die "$SO_PATH no es AArch64: $machine" ;;
    esac
fi

log "empaquetando el APK sin firmar"
(
    cd "$ANDROID_DIR"
    ./gradlew --no-daemon --console=plain \
        :app:assembleRelease \
        -PtargetAbi="$ABI" \
        -PappVersion="$APP_VERSION" \
        -PversionCode="$APP_VERSION_CODE" \
        -PunsignedRelease=true \
        -PcargoTargetDir="$TARGET_DIR"
)

[ -f "$UNSIGNED_APK" ] || die "no encuentro el APK sin firmar: $UNSIGNED_APK"

log "alineando y firmando"
rm -f "$SIGNED_APK" "$SIGNED_APK.idsig" "$SIGNED_APK.sha256"
"$ZIPALIGN" -p -f 4 "$UNSIGNED_APK" "$ALIGNED_APK"
"$APKSIGNER" sign \
    --ks "$KEYSTORE_FILE" \
    --ks-key-alias "$KEY_ALIAS" \
    --ks-pass env:RELOADED_KEYSTORE_PASSWORD \
    --key-pass env:RELOADED_KEY_PASSWORD \
    --v1-signing-enabled false \
    --v2-signing-enabled true \
    --v3-signing-enabled true \
    --v4-signing-enabled false \
    --min-sdk-version "$MIN_SDK" \
    --in "$ALIGNED_APK" \
    --out "$SIGNED_APK"
"$ZIPALIGN" -c -p 4 "$SIGNED_APK"

unset RELOADED_KEYSTORE_PASSWORD RELOADED_KEY_PASSWORD

log "verificando el APK"
"$APKSIGNER" verify --verbose --print-certs "$SIGNED_APK" > "$WORK_DIR/apksigner.txt"
CERT_SHA="$(sed -n 's/.*certificate SHA-256 digest: //p' "$WORK_DIR/apksigner.txt" | head -n 1)"
[ -n "$CERT_SHA" ] || die "apksigner no devolvio el SHA-256 del certificado"
if [ -n "${EXPECTED_CERT_SHA256:-}" ]; then
    [ "$CERT_SHA" = "${EXPECTED_CERT_SHA256,,}" ] || die "certificado inesperado: $CERT_SHA"
fi
if grep -qi 'certificate DN: CN=Android Debug' "$WORK_DIR/apksigner.txt"; then
    die "el APK quedo firmado con la clave de debug"
fi

BADGING="$("$AAPT2" dump badging "$SIGNED_APK")"
printf '%s\n' "$BADGING" | grep -Fq "package: name='$PACKAGE' versionCode='$APP_VERSION_CODE' versionName='$APP_VERSION'" ||
    die "el APK no declara $PACKAGE $APP_VERSION ($APP_VERSION_CODE)"
printf '%s\n' "$BADGING" | grep -Fq "minSdkVersion:'$MIN_SDK'" || die "minSdk inesperado"
printf '%s\n' "$BADGING" | grep -Fq "targetSdkVersion:'$TARGET_SDK'" || die "targetSdk inesperado"
if printf '%s\n' "$BADGING" | grep -q 'application-debuggable'; then
    die "el APK es debuggable"
fi
LAUNCH_COMPONENT="$(printf '%s\n' "$BADGING" | sed -n "s/^launchable-activity: name='\\([^']*\\)'.*/\\1/p" | head -n 1)"

LIBS="$(unzip -Z1 "$SIGNED_APK" | sed -n 's#^lib/\([^/]*\)/libgm8emulator.so$#\1#p' | sort)"
[ "$LIBS" = "$ABI" ] || die "el APK debe llevar solo lib/$ABI/libgm8emulator.so; lleva: ${LIBS:-ninguna}"
APK_SO_SHA="$(unzip -p "$SIGNED_APK" "lib/$ABI/libgm8emulator.so" | sha256sum | awk '{print $1}')"
STRIPPED_SO="$ANDROID_DIR/app/build/intermediates/stripped_native_libs/release/stripReleaseDebugSymbols/out/lib/$ABI/libgm8emulator.so"
[ -f "$STRIPPED_SO" ] || die "no encuentro el .so empaquetado: $STRIPPED_SO"
[ "$APK_SO_SHA" = "$(sha256sum "$STRIPPED_SO" | awk '{print $1}')" ] ||
    die "el .so dentro del APK no coincide con el que genero Gradle"

log "verificando el protocolo dentro del motor empaquetado"
EXTRACTED_SO="$WORK_DIR/libgm8emulator.so"
unzip -p "$SIGNED_APK" "lib/$ABI/libgm8emulator.so" > "$EXTRACTED_SO"
[ "$(sha256sum "$EXTRACTED_SO" | awk '{print $1}')" = "$APK_SO_SHA" ] ||
    die "el .so extraido del APK no coincide con el hash esperado"
strings -a "$EXTRACTED_SO" > "$WORK_DIR/engine-strings.txt"
for token in \
    "WVER 6" \
    "WBUILD 20261103" \
    "WOK side=" \
    "Zibelan conectado; esperando WOK" \
    "Zibelan: mundo conectado (lado " \
    "Bienvenido a " \
    "Zibelan conectando a " \
    "el rival salio" \
    "Reloaded Online desconectado"; do
    grep -Fq "$token" "$WORK_DIR/engine-strings.txt" ||
        die "el motor empaquetado no contiene el token de protocolo: $token"
done
PROTOCOL_STRINGS_STATUS="ok"

APK_SHA="$(sha256sum "$SIGNED_APK" | awk '{print $1}')"
APK_SIZE="$(stat -c '%s' "$SIGNED_APK")"
printf '%s  %s\n' "$APK_SHA" "$(basename "$SIGNED_APK")" > "$SIGNED_APK.sha256"
cp -f "$SO_PATH" "$OUT_DIR/libgm8emulator-$APP_VERSION-$ABI.so"
printf '%s  %s\n' "$SO_SHA" "libgm8emulator-$APP_VERSION-$ABI.so" > "$OUT_DIR/libgm8emulator-$APP_VERSION-$ABI.so.sha256"

INSTALL_STATUS="omitida"
LAUNCH_STATUS="no solicitado"
if [ "$INSTALL" -eq 1 ]; then
    [ -x "$ADB" ] || die "falta adb en $ADB"
    log "instalando en $("$ADB" get-serialno)"
    "$ADB" wait-for-device >/dev/null
    DEVICE_ABIS="$("$ADB" shell getprop ro.product.cpu.abilist | tr -d '\r')"
    case ",$DEVICE_ABIS," in
        *",$ABI,"*) ;;
        *) die "el dispositivo no soporta $ABI (abilist: $DEVICE_ABIS)" ;;
    esac

    BEFORE="$("$ADB" shell dumpsys package "$PACKAGE" 2>/dev/null | sed -n 's/.*versionCode=\([0-9]*\).*/\1/p' | head -n 1 | tr -d '\r')"
    if [ -n "$BEFORE" ] && [ "$BEFORE" -gt "$APP_VERSION_CODE" ]; then
        die "hay instalada una version mas nueva ($BEFORE); no se hace downgrade"
    fi

    "$ADB" install -r "$SIGNED_APK" >/dev/null
    INSTALL_STATUS="instalada"

    verified=0
    for _ in $(seq 1 20); do
        AFTER="$("$ADB" shell dumpsys package "$PACKAGE" 2>/dev/null | sed -n 's/.*versionCode=\([0-9]*\).*/\1/p' | head -n 1 | tr -d '\r')"
        if [ "$AFTER" = "$APP_VERSION_CODE" ]; then
            verified=1
            break
        fi
        sleep 1
    done
    [ "$verified" -eq 1 ] || die "el dispositivo no reporta versionCode $APP_VERSION_CODE"

    NAME="$("$ADB" shell dumpsys package "$PACKAGE" 2>/dev/null | grep -m 1 'versionName=' | sed 's/.*versionName=//' | awk '{print $1}' | tr -d '\r')"
    [ "$NAME" = "$APP_VERSION" ] || die "el dispositivo reporta versionName '$NAME'"
    "$ADB" shell dumpsys package "$PACKAGE" 2>/dev/null | grep -Fq "primaryCpuAbi=$ABI" ||
        die "el dispositivo no usa primaryCpuAbi=$ABI"

    PM_PATH="$("$ADB" shell pm path "$PACKAGE" | sed -n 's/^package://p' | tr -d '\r' | head -n 1)"
    if [ -n "$PM_PATH" ] && "$ADB" pull "$PM_PATH" "$WORK_DIR/installed-base.apk" >/dev/null 2>&1; then
        INSTALLED_SHA="$(sha256sum "$WORK_DIR/installed-base.apk" | awk '{print $1}')"
        [ "$INSTALLED_SHA" = "$APK_SHA" ] || die "el APK instalado no coincide con el firmado"
    else
        warn "no se pudo extraer el APK instalado; se omite la comparacion byte a byte"
    fi

    if [ "$LAUNCH" -eq 1 ]; then
        warn "el smoke test de arranque puede usar la red normal de la app (blog);"
        warn "no forma parte de la verificación offline del protocolo."
        if [ -n "$LAUNCH_COMPONENT" ]; then
            "$ADB" shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
            "$ADB" logcat -c >/dev/null 2>&1 || true
            "$ADB" shell am start -W -n "$PACKAGE/$LAUNCH_COMPONENT" >/dev/null
            alive=0
            for _ in $(seq 1 20); do
                if "$ADB" shell pidof "$PACKAGE" >/dev/null 2>&1; then
                    alive=1
                    break
                fi
                sleep 1
            done
            [ "$alive" -eq 1 ] || die "la app no sigue viva tras lanzarla"
            sleep 2
            if "$ADB" logcat -d -t 400 | grep -Eq 'FATAL EXCEPTION|AndroidRuntime.*'"$PACKAGE"; then
                "$ADB" logcat -d -t 400 | tail -n 80 >&2
                die "la app registro un crash al arrancar"
            fi
            LAUNCH_STATUS="ok"
        fi
    fi
fi

cat > "$OUT_DIR/release-$APP_VERSION.txt" <<EOF
version=$APP_VERSION
version_code=$APP_VERSION_CODE
package=$PACKAGE
abi=$ABI
git_commit=$GIT_FULL
git_branch=$GIT_BRANCH
checkout_path=$ROOT
source_date_epoch=$SOURCE_DATE_EPOCH
apk=$(basename "$SIGNED_APK")
apk_size=$APK_SIZE
apk_sha256=$APK_SHA
engine_so=libgm8emulator-$APP_VERSION-$ABI.so
engine_so_sha256=$SO_SHA
apk_so_sha256=$APK_SO_SHA
certificate_sha256=$CERT_SHA
protocol_tests=$PROTOCOL_TEST_STATUS
protocol_test_detail=$PROTOCOL_DETAIL
protocol_strings=$PROTOCOL_STRINGS_STATUS
install=$INSTALL_STATUS
launch=$LAUNCH_STATUS
EOF

log "listo"
printf 'APK:       %s\n' "$SIGNED_APK"
printf 'SHA-256:   %s\n' "$APK_SHA"
printf 'SO:        %s (%s)\n' "$SO_PATH" "$SO_SHA"
printf 'Protocolo: tests=%s, strings=%s\n' "$PROTOCOL_TEST_STATUS" "$PROTOCOL_STRINGS_STATUS"
printf 'Cert:      %s\n' "$CERT_SHA"
printf 'Manifiesto: %s\n' "$OUT_DIR/release-$APP_VERSION.txt"
