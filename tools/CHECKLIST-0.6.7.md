# 0.6.7 build/install verification (0.6.3 protocol integration)

Scope: `horde/z-build63`, package `com.klisee.reloaded`, version `0.6.7`, fixed
`versionCode=67`, ABI `arm64-v8a`.

The build is verified against the protocol integration documented in
`docs/zibelan-protocol-re.md`:

- the world handshake is `WJOIN <name>`, `WVER 8`, `WBUILD 20261109`, then
  `WSYNC`;
- `WELCOME <u64>` is the session identity and the welcome notice;
- `WOK <u64>` acknowledges the joined world without inventing an identity;
- `PEER_LEFT` clears the interaction peer without deleting the world record;
- the packaged engine still contains those command literals and the Spanish
  connection notices.

The parser and host checks are offline. The engine tests use mock transports
bound to `127.0.0.1:0`; this checklist never contacts a live Reloaded or
Zibelan server.

## Offline rules

- No live server probe, packet capture, credential, or endpoint is used.
- No game data, GML, F3R assets or derived F3R IDs are used. The same
  derived-GML guard as the CI workflow runs in preflight.
- Install verification does not launch the app by default. `--launch` is an
  optional smoke test; it is outside this offline protocol verification and the
  app may use its normal network paths (for example, the blog feed) when
  launched.
- Do not bump the source defaults in `android/app/build.gradle.kts`
  (`0.4.1` / `18`). The release script pins
  `-PappVersion=0.6.7 -PversionCode=67`.
- This build change only touches `.gitignore`, `android/app/build.gradle.kts`
  and `tools/`.
- Use a shared Cargo target directory only for checks (`cargo check` /
  `cargo test`). Never build or package the release into a shared target: it can
  mix artifacts from other worktrees.

## Pinned inputs

| Input | Expected |
| --- | --- |
| Commit | output of `git rev-parse HEAD` at build time |
| Rust | `1.99.0` (`src/OpenGMK/rust-toolchain.toml`) |
| Gradle | `9.4.1` (wrapper) |
| JDK | 17 or newer |
| Android build-tools | `37.0.0` |
| Android NDK | `27.x` (script picks the newest installed one) |
| Android platform for cargo-ndk | `29` |
| Device | `arm64-v8a`, API 26+, adb authorised |
| Signing key | release key, or a local test key for test builds |

Set `EXPECTED_CERT_SHA256` to the fingerprint of the signer you intend to use
if the build must reject a different certificate. The script always records the
actual certificate SHA-256 in the manifest.

## Preflight

```bash
cd /tmp/opencode/z-build63
git status --porcelain                  # must be empty
bash -n tools/build-067.sh

# Must print nothing (same guard as the CI workflow):
! grep -R -n -E 'global\.mods_available|file_exists\("mods/|text_op5|draw_text_color\(160|var y0,h,step|mouse_x>=80 && mouse_x<=240' \
    --include='*.rs' src/OpenGMK/gm8emulator/src

# Protocol checks only, with a shared check target (never used for packaging):
CARGO_TARGET_DIR=/tmp/opencode/proto-check-target \
    cargo test --locked -p reloaded-cleanroom --lib handshake
CARGO_TARGET_DIR=/tmp/opencode/proto-check-target \
    cargo test --locked -p reloaded-cleanroom --lib welcome_and_ok
CARGO_TARGET_DIR=/tmp/opencode/proto-check-target \
    cargo test --locked -p gm8emulator --lib welcome
CARGO_TARGET_DIR=/tmp/opencode/proto-check-target \
    cargo test --locked -p gm8emulator --lib wok
CARGO_TARGET_DIR=/tmp/opencode/proto-check-target \
    cargo test --locked -p gm8emulator --lib peer_left
```

Expected results at this commit:

```text
roz::tests::handshake_is_three_commands_in_order ... ok
test result: ok. 1 passed; 0 failed

roz::tests::welcome_and_ok_use_full_u64_ids ... ok
test result: ok. 1 passed; 0 failed

reloaded_online::tests::json_welcome_is_single_notice_across_reconnect ... ok
reloaded_online::tests::mock_transport_malformed_welcome_keeps_fallback_parse ... ok
reloaded_online::tests::mock_transport_welcome_shows_id_and_version_once ... ok
test result: ok. 3 passed; 0 failed

game::external::roz_net::tests::status_side_and_peer_follow_wait_start_peer_left_and_wok ... ok
reloaded_online::tests::mock_transport_self_updates_are_suppressed_after_wok ... ok
test result: ok. 2 passed; 0 failed

reloaded_online::tests::peer_left_and_remote_left_are_reported_once_for_trade_cancellation ... ok
game::external::roz_net::tests::status_side_and_peer_follow_wait_start_peer_left_and_wok ... ok
reloaded_online::tests::zibelan_peer_left_reports_a_clear_message_without_dropping_the_world ... ok
game::external::roz_net::tests::input_get_handles_input_timeout_epoch_and_peer_left ... ok
test result: ok. 4 passed; 0 failed
```

The script runs those five filters itself and records the totals.

## Build and sign

```bash
cd /tmp/opencode/z-build63
export KEYSTORE_PROPERTIES=/tmp/opencode/test-067.properties
tools/build-067.sh --no-install --out /tmp/opencode/dist-proto-067-a
```

The script:

1. refuses a dirty tree;
2. runs the five offline protocol filters and requires at least
   `1 + 1 + 3 + 2 + 4 = 11` passing tests with zero failures;
3. runs `cargo ndk ... build --release --locked` for `aarch64-linux-android`;
4. packages with `unsignedRelease=true`, zipaligns, and signs with apksigner
   (v2/v3, no debug key);
5. verifies the APK manifest and that the packaged `.so` contains the protocol
   command literals and connection notice strings.

Expected values in `/tmp/opencode/dist-proto-067-a/release-0.6.7.txt`:

```text
version=0.6.7
version_code=67
package=com.klisee.reloaded
abi=arm64-v8a
apk=pokemon-reloaded-0.6.7-arm64-v8a.apk
protocol_tests=ok (11 passed en 5 filtros)
protocol_test_detail=handshake=1, welcome_codec=1, welcome_host=3, wok_host=2, peer_left_host=4
protocol_strings=ok
```

`--skip-protocol-tests` exists only for constrained hosts. It leaves
`protocol_tests=omitidos` in the manifest and is not an accepted verification
run.

## Artifact checks

```bash
cd /tmp/opencode/dist-proto-067-a
sha256sum -c ./*.sha256

APK=pokemon-reloaded-0.6.7-arm64-v8a.apk
unzip -Z1 "$APK" | grep 'lib/.*/libgm8emulator.so'   # only lib/arm64-v8a/...
"$ANDROID_SDK_ROOT/build-tools/37.0.0/aapt2" dump badging "$APK" | \
    grep -E "package:|sdkVersion|targetSdkVersion|application-debuggable"
"$ANDROID_SDK_ROOT/build-tools/37.0.0/apksigner" verify --verbose --print-certs "$APK"

# The packaged engine still carries the protocol surface:
unzip -p "$APK" lib/arm64-v8a/libgm8emulator.so > /tmp/libgm8emulator-067.so
strings -a /tmp/libgm8emulator-067.so | grep -F \
    -e 'WVER 8' \
    -e 'WBUILD 20261109' \
    -e 'WOK side=' \
    -e 'Zibelan conectado; esperando WOK' \
    -e 'Zibelan: mundo conectado (lado ' \
    -e 'Bienvenido a ' \
    -e 'Zibelan conectando a ' \
    -e 'el rival salio' \
    -e 'Reloaded Online desconectado'
```

Acceptance:

- `package: name='com.klisee.reloaded' versionCode='67' versionName='0.6.7'`
- `sdkVersion:'26'`, `targetSdkVersion:'36'`
- no `application-debuggable`
- certificate SHA-256 matches the expected signer, not `CN=Android Debug`
- only `lib/arm64-v8a/libgm8emulator.so` is packaged
- `apk_so_sha256` in the manifest matches the `.so` extracted from the APK
- all nine protocol/notice strings are present in the packaged `.so`

## Determinism

Same checkout, same toolchain and same inputs must produce the same hashes:

```bash
cd /tmp/opencode/z-build63
export KEYSTORE_PROPERTIES=/tmp/opencode/test-067.properties
tools/build-067.sh --no-install --out /tmp/opencode/dist-proto-067-a
tools/build-067.sh --no-install --out /tmp/opencode/dist-proto-067-b
sha256sum /tmp/opencode/dist-proto-067-{a,b}/*.apk \
          /tmp/opencode/dist-proto-067-{a,b}/*.so
```

The APK SHA-256 and engine `.so` SHA-256 must match between `-a` and `-b`.

Stronger clean-rebuild check:

```bash
rm -rf src/OpenGMK/target android/app/build android/.gradle
tools/build-067.sh --no-install --out /tmp/opencode/dist-proto-067-clean
sha256sum /tmp/opencode/dist-proto-067-{a,clean}/*.apk \
          /tmp/opencode/dist-proto-067-{a,clean}/*.so
```

Note: the release `.so` keeps absolute source paths in panic strings. Two
different worktree paths can therefore produce different bytes even with
identical source. Compare only builds made from the same checkout path
(`checkout_path` in `release-0.6.7.txt` is recorded for this reason).

## Install verification

The script installs by default (omit `--no-install`), but does not launch:

```bash
cd /tmp/opencode/z-build63
export KEYSTORE_PROPERTIES=/tmp/opencode/test-067.properties
tools/build-067.sh --out /tmp/opencode/dist-proto-067
```

It checks the device ABI list, refuses to downgrade an installed
`versionCode > 67`, runs `adb install -r`, waits for the device to report
`versionCode=67` and `versionName=0.6.7`, checks
`primaryCpuAbi=arm64-v8a`, and compares the pulled installed APK with the signed
APK when possible.

If the installed app is signed with another key (for example the production
key), `adb install -r` fails with a signature mismatch. Do not uninstall a
user's app just to run this checklist; install on a test device or with the
matching key instead.

Manual confirmation:

```bash
adb shell dumpsys package com.klisee.reloaded | \
    grep -E 'versionCode|versionName|primaryCpuAbi|lastUpdateTime'
adb shell pm path com.klisee.reloaded
```

Expected: `versionCode=67`, `versionName=0.6.7`, `primaryCpuAbi=arm64-v8a`, and
`install=instalada`, `launch=no solicitado` in `release-0.6.7.txt`.

An optional launch smoke test is `--launch`. It is not part of the offline
protocol verification because the app may use the network after starting, and
because the protocol itself only runs when the engine connects to a server
(excluded from this task).

## Gaps

- Only `arm64-v8a` is built and installed. `universal`, `armeabi-v7a` and
  `x86_64` are not covered by this checklist.
- No live-server end-to-end test: the real Zibelan `WELCOME`/`WOK` wire flow is
  not exercised. Protocol behavior is checked with local loopback mocks plus the
  packaged string surface only.
- A device was attached during preparation, but install was not executed: the
  installed 0.6.7 APK is signed with a different key than the local test key.
  Replacing it would require uninstalling the user's app, which is out of scope.
  `--no-install` covers build/artifact verification; install remains scripted
  and unexecuted here.
- `--launch` is opt-in and may touch the app's normal network paths (blog feed).
  It is a process-alive smoke test, not protocol verification.
- Install requires the same signing key as the installed app. Test builds use a
  test key; the production key is not in the repository.
- Byte-for-byte reproducibility across different checkout paths is not guaranteed
  because the `.so` contains absolute source paths. It is only checked within one
  checkout path.
- Gradle/Cargo caches can hide non-determinism. The clean rebuild above is the
  strong check.
- The protocol tests are host tests. A constrained host can skip them with
  `--skip-protocol-tests`, but then the protocol acceptance is not met.
- The script does not tag, publish or create a GitHub release.
