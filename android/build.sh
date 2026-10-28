#!/bin/bash
# Build the Android app with the checked-in Gradle wrapper. The old script depended on a Gradle
# distribution happening to be cached under a hard-coded version, which made clean checkouts and CI
# fail. Keep a local-cache fallback only for machines that already have that distribution.
set -e
cd "$(dirname "$0")"

if [ -x ./gradlew ]; then
    exec ./gradlew "$@"
fi

GRADLE_DIST="${GRADLE_DIST:-9.4.1}"
GRADLE=$(ls -d "$HOME"/.gradle/wrapper/dists/gradle-"$GRADLE_DIST"-bin/*/gradle-"$GRADLE_DIST"/bin/gradle 2>/dev/null | head -1)
if [ -z "$GRADLE" ]; then
    echo "no encuentro ./gradlew ni Gradle $GRADLE_DIST en la caché; define GRADLE=/ruta/a/gradle"
    exit 1
fi

exec "$GRADLE" "$@"
