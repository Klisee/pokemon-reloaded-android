# 0.6.3 / Zibelan protocol merge plan and acceptance matrix

Status: read-only merge analysis for the nine `horde/z-*` tips based on
`main@6012b1e` (2026-11-08). This branch commits this document only; no
production branch, tag, release or worktree was changed. The merge sequence
below was exercised in a throwaway clone whose tip is `2a1ac9b` (not a
production ref).

Scope note: "0.6.3" here is the Zibelan protocol profile extracted from
`/tmp/z063/net_scripts.gml` (49 online scripts), not the Android app version.
`/tmp/z063/scripts.gml` initialises `global.NET_VERSION="8"` and
`global.NET_BUILD=20261109`. The build tip targets app `0.6.7`, versionCode
`67`.

## 1. Frozen inputs

| Branch | Commit | Subject |
| --- | --- | --- |
| `horde/z-handshake63` | `d04b31d` | online: handshake Zibelan con WTID/WTSPR y WVER 8 |
| `horde/z-cfg63` | `001e159` | wcfg: codec limpio de 37 campos y exchange tras TEAM |
| `horde/z-sec63` | `e56cc8a` | sec: validar campos y anti-replay del protocolo 0.6.3 |
| `horde/z-sync63` | `8773c42` | online: sincronizar IN/SUM, semilla y BYE del combate |
| `horde/z-host63` | `f8fbf57` | online: integrar configuracion de combate 0.6.3 en host |
| `horde/z-ui63` | `3025e30` | ui: avisos de conexión y combate 0.6.3 |
| `horde/z-tests63` | `034e0b1` | test: cubrir flujo online 0.6.3 con transporte simulado |
| `horde/z-doc63` | `5caec47` | docs: documentar protocolo Zibelan 0.6.3 desde GML estático |
| `horde/z-build63` | `d127c8a` | build: verificación determinista 0.6.7 del protocolo 0.6.3 |

All nine tips are descendants of `6012b1e`. The reference documents are
`docs/zibelan-063-protocol.md` (new in `z-doc63`) and the updated
`docs/zibelan-protocol-re.md` (in `z-handshake63`).

## 2. Merge order

Use `--no-ff` so every feature tip remains visible. Do not merge this batch
into `main` from `horde/z-merge63`; use a dedicated release integration branch.

```bash
git checkout -b integrate/z063 main        # or the release branch
git merge --no-ff --no-edit horde/z-handshake63
git merge --no-ff --no-edit horde/z-cfg63
git merge --no-ff --no-edit horde/z-sec63
git merge --no-ff --no-edit horde/z-sync63
git merge --no-ff --no-edit horde/z-host63
git merge --no-ff --no-edit horde/z-ui63
git merge --no-ff --no-edit horde/z-tests63
git merge --no-ff --no-edit horde/z-doc63
git merge --no-ff --no-edit horde/z-build63
```

| Step | Branch | Why this position |
| --- | --- | --- |
| 1 | `z-handshake63` | Establishes `WVER 8` / `WBUILD 20261109`, `WTID`/`WTSPR` and the bounded world-handshake wait. Everything else reads this protocol baseline. |
| 2 | `z-cfg63` | Adds the clean-room 37-field `WCFG` codec. Independent module; before the host so later branches can refer to the finalized field table. |
| 3 | `z-sec63` | Adds `z063_sec` validators and the `online` re-export. Must land after the handshake constants it validates. Shares only adjacent `lib.rs`/README lines with `z-cfg63`. |
| 4 | `z-sync63` | Adds `BattleSync`, delayed `IN`, 30-frame `SUM`, `BYE` and frame seeding. Needs the protocol baseline; the host and UI branches build on this battle loop. |
| 5 | `z-host63` | Adds runtime `WCFG` send/apply and host battle metadata. Depends on the sync battle flow; its only textual conflict is the delay expression in `game.rs`. |
| 6 | `z-ui63` | Adds native notices and desync copy. After sync/host so the notice hooks sit on the final battle paths and `failure.rs` gains `Desync`. |
| 7 | `z-tests63` | Adds `online/tests/z063_flow.rs` plus host test additions. Must be after the APIs it exercises; keeps its test conflict local to `reloaded_online.rs`. |
| 8 | `z-doc63` | New protocol note only. Can move earlier or later without affecting code; keep it after code so the merge history reads protocol first, documentation last. |
| 9 | `z-build63` | Release tooling and version properties. Must be last because its gates assert the final engine strings; see the mandatory correction in section 4. |

## 3. Conflict map

### 3.1 Textual conflicts in the validated order

Only two files conflict in the order above.

| Step | File | Conflict | Resolution |
| --- | --- | --- | --- |
| 5 (`z-host63`) | `src/OpenGMK/gm8emulator/src/game.rs` | In `battle_team_meta`, `HEAD` (sync) has `let delay = crate::battle_sync::initial_delay(self.global_number(f3r::global::NET_DELAY, 0).unwrap_or(2));`; `z-host63` has `let delay = self.global_number(...).unwrap_or(2).clamp(2, 10);`. | Keep `battle_sync::initial_delay(...)`. It is the same 2..=10 clamp and keeps one source of truth for the delay rule. |
| 7 (`z-tests63`) | `src/OpenGMK/gm8emulator/src/reloaded_online.rs` | Adjacent test functions in `mod tests`: `HEAD` has the host `mock_transport_battle_config_round_trips` and `mock_transport_invalid_battle_config_reports_error`; `z-tests63` adds `mock_transport_063_team_version_mismatch_is_dropped`. The shared closing brace makes Git present this as one conflict. | Keep all three tests. Add the missing `}` after the invalid-config test and keep `#[test]` before the `z-tests63` test. No test may be dropped. |

No other textual conflicts were produced. In particular, `z-cfg63` and
`z-sec63` both append to `reloaded-cleanroom/src/lib.rs` and its README; in this
order Git merges both cleanly (keep both module lines/bullets).

### 3.2 Auto-merged overlaps that need review

| File | Branches | What to verify after the merge |
| --- | --- | --- |
| `src/OpenGMK/gm8emulator/src/reloaded_online.rs` | handshake, sync, host, ui, tests | Struct fields and `Default` contain all of `world_handshake_timeout`, `world_trainer_id`, `world_sprite`, `battle_sim_frame`, `battle_checksums`, `battle_bye_frame`, `battle_configs`, `battle_config_error`, `notice`. `begin_battle`/`begin_battle_with_delay`/`end_battle`/`stop` initialise or clear them. |
| `src/OpenGMK/gm8emulator/src/game.rs` | handshake, sync, host, ui | `start_zibelan_world` uses the save identity; `battle_team_meta` uses `initial_delay`; `battle_sync_tick_zibelan` runs before the legacy input loop; WCFG send/apply hooks are reachable; UI notices are drained outside boot/loader/select rooms. |
| `src/OpenGMK/reloaded-cleanroom/src/lib.rs`, `README.md` | cfg, sec | Both `pub mod wcfg;` and `pub mod z063_sec;` (and both README bullets) survive. |
| `docs/zibelan-protocol-re.md` | handshake | The document now describes `WVER 8` / `WBUILD 20261109`, while older captures keep the legacy three-line form. Do not let the obsolete `WVER 6` text propagate into the build checklist. |
| `tools/` | build | `tools/build-067.sh` and `tools/CHECKLIST-0.6.7.md` still contain the pre-handshake string gate; see section 4. |

### 3.3 Files that merge cleanly

`docs/zibelan-063-protocol.md`, `docs/zibelan-protocol-re.md`,
`src/OpenGMK/online/src/roz.rs`, `src/OpenGMK/reloaded-cleanroom/src/roz.rs`,
`src/OpenGMK/online/src/failure.rs`, `src/OpenGMK/online/tests/z063_flow.rs`,
`src/OpenGMK/gm8emulator/src/battle_sync.rs`,
`src/OpenGMK/reloaded-cleanroom/src/wcfg.rs`,
`src/OpenGMK/reloaded-cleanroom/src/z063_sec.rs`,
`src/OpenGMK/reloaded-cleanroom/src/z063_sec_tests.rs`,
`src/OpenGMK/gm8emulator/src/lib.rs`, `.gitignore`,
`android/app/build.gradle.kts`, `tools/build-067.sh`,
`tools/CHECKLIST-0.6.7.md`.

## 4. Mandatory correction before the release gate

`z-build63` was written against the older three-line handshake. After
`z-handshake63`, the production encoder writes `WVER 8` / `WBUILD 20261109`
(and writes the numbers at runtime rather than as one literal). The packaged
engine will therefore **not** contain the literals `WVER 6` or
`WBUILD 20261103`, and `tools/build-067.sh` aborts in its `strings` gate.

Required edit in the integration branch (not done in this doc commit):

- `tools/build-067.sh:395-396`: replace the two version-specific tokens with
  prefix tokens `WVER ` and `WBUILD ` (or with `WVER 8` /
  `WBUILD 20261109` only if a local build proves the linker keeps those exact
  strings). The robust gate is `WVER ` plus the
  `world_handshake_sends_net_world_connect_lines_in_order` test.
- `tools/CHECKLIST-0.6.7.md:9` and `:159-160`: update the documented handshake
  to `WJOIN`, `WTID`, optional `WTSPR`, `WVER 8`, `WBUILD 20261109`, and update
  the expected filter counts (actual: handshake 7, `welcome_and_ok` 1,
  welcome 3, wok 2, `peer_left` 4 = 17 total; the script's minimum of 11 still
  passes).

This is the only known release-blocking post-merge edit. All Rust tests pass
without it; only the APK string gate fails.

## 5. Acceptance test matrix

Legend: **PASS** = executed in the throwaway merged clone
(`2a1ac9b`); **PENDING** = requires an arm64 device/emulator or a controlled
server; **BLOCKED** = known gap that must be fixed or explicitly waived.

### 5.1 Automated gate (exact commands and results)

Run from the merged checkout's `src/OpenGMK` unless stated otherwise.

| ID | Command | Expected | Result |
| --- | --- | --- | --- |
| A1 | `cargo test --workspace --locked --offline` | 0 failures; 662 tests passed: gm8emulator 312, reloaded-cleanroom 240, reloaded-online 64, z063_flow 7, gml-parser 39 (plus 0-test binaries/doc-tests) | PASS |
| A2 | `cargo test --locked --offline -p gm8emulator --lib` | `312 passed; 0 failed` | PASS |
| A3 | `cargo test --locked --offline -p reloaded-cleanroom --lib` | `240 passed; 0 failed` | PASS |
| A4 | `cargo test --locked --offline -p reloaded-online --lib` | `64 passed; 0 failed` | PASS |
| A5 | `cargo test --locked --offline -p reloaded-online --test z063_flow` | `7 passed; 0 failed` | PASS |
| A6 | `cargo test --locked --offline -p gml-parser --lib` | `39 passed; 0 failed` | PASS |
| A7 | `git diff --check 6012b1e..HEAD` | no output | PASS |
| A8 | `bash -n tools/build-067.sh` | exit 0 | PASS |
| A9 | grep for conflict markers in `src/`, `docs/`, `tools/` | none | PASS |
| A10 | `tools/build-067.sh --no-install --out <dir> --keystore-properties <file>` (clean tree) | sections 4 and 5.1: protocol filters pass, APK versionCode 67/versionName 0.6.7, minSdk 26, targetSdk 36, one `lib/arm64-v8a/libgm8emulator.so`, v2+v3 signatures, expected cert | BLOCKED until the token edit in section 4 |

### 5.2 Targeted protocol/feature filters (exact)

| ID | Command filter | Expected | Result |
| --- | --- | --- | --- |
| B1 | `-p reloaded-cleanroom --lib handshake` | `7 passed; 0 failed` (script minimum 1) | PASS |
| B2 | `-p reloaded-cleanroom --lib welcome_and_ok` | `1 passed; 0 failed` | PASS |
| B3 | `-p gm8emulator --lib welcome` | `3 passed; 0 failed` | PASS |
| B4 | `-p gm8emulator --lib wok` | `2 passed; 0 failed` | PASS |
| B5 | `-p gm8emulator --lib peer_left` | `4 passed; 0 failed` | PASS |
| B6 | `-p gm8emulator --lib zibelan_start_world` | `5 passed; 0 failed` | PASS |
| B7 | `-p gm8emulator --lib battle_config` | `8 passed; 0 failed` | PASS |
| B8 | `-p gm8emulator --lib battle_sync::tests` | `13 passed; 0 failed` | PASS |
| B9 | `-p reloaded-cleanroom --lib wcfg` | `33 passed; 0 failed` | PASS |
| B10 | `-p reloaded-cleanroom --lib z063_sec` | `34 passed; 0 failed` | PASS |
| B11 | `-p reloaded-online --lib failure` | `6 passed; 0 failed` | PASS |

Required named regressions in the merged tree:

- `reloaded_online::tests::zibelan_start_world_sends_net_world_connect_handshake`
  must send exactly `WJOIN`, `WTID`, `WTSPR` (optional), `WVER 8`,
  `WBUILD 20261109`.
- `reloaded_online::battle_config_tests::*` and
  `mock_transport_battle_config_round_trips` must round-trip all 37 fields.
- `battle_sync::tests::*` must cover initial delay, frame seed,
  input delay, 30-frame checksums, BYE and delay adaptation.
- `mock_transport_battle_initial_inputs_and_30_frame_checksums` and
  `mock_transport_battle_sum_and_bye_messages_are_epoch_scoped` must pass.
- `z063_flow` must pass all 7 static-GML flow tests
  (`world_handshake`, status 6/5/failure, TEAM five fields, WCFG 37 fields,
  delay/IN/SUM/BYE/checksum).

### 5.3 Device/manual acceptance (arm64, controlled server; never the public endpoint)

All cases: two devices A/B with the same signed APK, same `host:port` and room,
no proxy/VPN, `logcat -c` before each case, capture threadtime logs, and verify
no `FATAL EXCEPTION`, `panicked at`, or `Fatal signal`.

| ID | Scenario | Exact steps | Expected |
| --- | --- | --- | --- |
| D1 | World handshake and identity | A connects alone, then B. Capture the handshake lines and logcat. | Order is `WJOIN <16-byte name>`, `WTID <digits>`, optional `WTSPR <sprite>`, `WVER 8`, `WBUILD 20261109`; first `WSYNC` follows; `WELCOME <id>`/`WOK <id>` adopt the same id; no `linea Zibelan invalida`; no id #0. |
| D2 | Full world | Connect to a room kept at capacity. | Status 5 maps to the "mundo lleno" notice once; no retry storm; client returns to offline play without crash. |
| D3 | World presence | Both players walk across rooms; press G; chat; emote. | `WP`/`WDEL` add/remove one avatar per id; `WP` wrong-room updates are ignored; `WLIST` roster is accurate and paginates; `WSAY`/`WEXC` show once; no ghost/duplicate avatars after room change. |
| D4 | Challenge F/T/G | From A's interaction menu choose Combatir, Intercambiar, Regalar; repeat A-first and B-first. | `WREQ <id> <kind>` is sent; B gets one native request; `WANS 1` starts the session; `WANS 0`/`WANS 2`/timeout terminate with the matching message; cancel sends `WCANCEL`; no request is applied twice. |
| D5 | Gift with full party | Fill B's party to six, then A sends a gift. | B answers `WANS 4` without opening the gift flow; message says the party is full; A receives the full-party response; no Pokémon/items are moved. |
| D6 | TEAM metadata | Start a battle in both host/guest order. | Five RS fields (seed, fondo, modo, sala, delay) precede the `.TEAM` body; guest clamps delay to 2..=10; a short/malformed prefix produces `NO` and the version-mismatch message; teams load exactly once. |
| D7 | WCFG authority | Host changes background/mode/weather before the battle; guest joins. | Host sends `WCFG` with 37 GS fields; guest applies all fields, including day/night hour/minute and the five text fields; a background change calls `MultiplayerSetBackground`; guest never counter-offers. |
| D8 | Lockstep IN/SUM/BYE | Play at least 150 frames; then leave normally; repeat with a forced peer kill. | Empty `IN` frames fill 1..=delay; local input frame f is applied at f+delay; `SUM` is sent every 30 frames with the GML checksum; `BYE <epoch> <frame>` ends the peer after that frame; a killed socket ends the battle without hanging. |
| D9 | Desync visibility | Force divergent state on one side (test build or injected checksum). | Exactly one desync notice/status per battle; no duplicate notices; per `NetDesync` the battle may continue, but it must not be a silent log-only event. **Known gap:** the merged Zibelan fast path sets `battle_desync` but does not call `note_battle_desync()` or show a notice; the notice branch is only in the legacy path. See R2. |
| D10 | Reconnect | Enable `data/text/mundo_reconectar.txt`; kill the server socket; restore it. | Backoff `min(60000, 3000 * 2^min(N,5))` ms; reconnect resends the five-line handshake; waits up to 6 s for status 6; clears world map/remotes/requests/trade/battle queues; one "reconectado" note; no stale `OFFER`/`OK` or old avatar. |
| D11 | Notice/input latch | Trigger connected, full, no-response, version-mismatch and disconnect notices; click/accept each. | One notice slot at a time; Aceptar consumes the press; no click-through into the title/menu/GML; notices do not block `reloaded_online.tick`; status copy matches `OnlineNoticeKind`. |
| D12 | APK/build | Install the signed arm64 APK from section 4/5.1. | `com.klisee.reloaded` versionName `0.6.7`, versionCode `67`, minSdk `26`, targetSdk `36`, only `arm64-v8a`, v2+v3 signed, expected cert, launch smoke test alive, no FATAL. |

### 5.4 Security/negative matrix (unit level, `reloaded-cleanroom --lib z063_sec`)

| ID | Case | Expected |
| --- | --- | --- |
| S1 | `WTID` empty / non-digit / >10 digits / >u32 | `TrainerIdEmpty`, `TrainerIdNotDigits`, `TrainerIdTooLong`, `TrainerIdOverflow` |
| S2 | `WTSPR` below -1 or above 3856 | `SpriteOutOfRange`; -1 and 0..=3856 accepted |
| S3 | `WVER`/`WBUILD` not exactly `(6,20261103)` or `(8,20261109)` | `VersionBuildMismatch` / `UnsupportedVersion` |
| S4 | Missing `WTID` on v8, sprite without trainer id, command out of order, duplicate field | matching `HandshakeError`, no partial state |
| S5 | WCFG out-of-range numbers, control bytes, non-canonical decimals, short/oversized payload | rejection; 37-field wire and up to 40 fields accepted |
| S6 | TEAM missing one of five metadata fields, empty body, non-canonical numbers, oversized body | rejection; empty body after five fields accepted |
| S7 | Replayed/stale/future `SUM`, wrong frame, contradictory local checksum | no false desync except a same-frame mismatch; one desync signal per frame |
| S8 | `BYE` with wrong epoch, replay, frame outside the window | ignored/rejected; valid epoch+frame accepted once |

### 5.5 Coverage gaps versus `net_scripts.gml` (not changed by this batch)

These scripts have no corresponding `z-*` change and need regression-only
coverage or a future batch: tournament (`WTOPEN`/`WTMATCH`/`WTVER`/`WTCUADRO`/
`WTPREMIO`/`WTINFO`), `NetSpectate`, `NetSaveRepair`/`NetMasterMargin`,
`NetDesyncReport`/`WDESYNC` server handling, full `NetWorldParts`/avatar
rendering, and the JSON backend path. Do not treat the 0.6.3 acceptance above
as proof for those areas.

## 6. Risks

| Rank | Risk | Evidence | Mitigation / gate |
| --- | --- | --- | --- |
| R1 (blocker) | `z-build63` string gate is stale after `z-handshake63`; release APK build aborts at `strings`. | `tools/build-067.sh:395-396` checks `WVER 6`/`WBUILD 20261103`; production now encodes `WVER 8`/`WBUILD 20261109`; those literals exist only in tests/comments after the merge. | Apply the section 4 edit before A10; re-run the five protocol filters and the string gate. |
| R2 (high) | Zibelan desync is log-only; the UI notice branch is unreachable for Zibelan. | `game.rs` returns into `battle_sync_tick_zibelan` before the legacy checksum branch that calls `note_battle_desync()`; the Zibelan path only `eprintln!`s and sets `battle_desync`. | Wire the Zibelan desync path to `note_battle_desync()`/`take_notice()` and add a test; or explicitly waive D9. Do not claim D9 passes as merged. |
| R3 (high) | `z-cfg63` and `z-sec63` are library-only; the host uses its own `BattleConfig` and never calls `wcfg`/`z063_sec`. | `online/src/lib.rs` only re-exports `z063_sec`; `reloaded_online.rs` defines a second 37-field `BattleConfig`; no runtime call to `wcfg::`/`z063_sec::` in `gm8emulator`. | Either wire the clean-room codec/validators into the host or record two intentionally separate implementations and keep their field tables/test vectors in sync. Add a cross-check test. |
| R4 (medium) | `z-tests63` exercises `RozClient` directly, not the `Game`/`ReloadedOnline` WCFG host flow. | `online/tests/z063_flow.rs` builds frames by hand; host tests are separate mock-transport tests. | Keep both; require D7/D8 device runs and the host `battle_config` filters (B7). |
| R5 (medium) | Strict WCFG behaviour differs from GML: `NetConfigApply` silently ignores a short payload; the host returns an error and side 2 requires a config. | GML `NetConfigApply` exits when fewer than 36 separators; `game.rs` returns `WCFG` `FunctionError` when side 2 has no config. | Decide compatibility policy; test short/missing WCFG on both sides and ensure a clear terminal message, never a silent half-applied battle. |
| R6 (medium) | Version/build provenance is split: protocol says `8/20261109`, release tooling/checklist still say app `0.6.7`/`67` and legacy `6/20261103`. | `z-doc63` section 10, `z-handshake63` constants, `z-build63` checklist. | Make the release manifest state both values explicitly; update checklist and build tokens. |
| R7 (medium) | GML does not parse `WELCOME`/`WOK`; our implementation waits for them before status 6. | `docs/zibelan-063-protocol.md` sections 3.3 and 11.2; `wait_world_status` in `online/src/roz.rs`. | Against the controlled server, verify the actual response sequence; keep the 5 s/6 s bounds and a terminal timeout path. Do not assume a GML-status-only server works. |
| R8 (low) | Shared worktrees were actively edited during analysis; uncommitted state can be mistaken for a branch tip. | All nine tips are now committed and clean; simulation used `origin/horde/*` refs. | Re-check `git status` and the SHAs in section 1 before merging. |
| R9 (low) | Auto-merged `reloaded_online.rs`/`game.rs` hunks can hide semantic loss even when Git reports clean. | Four branches add fields/helpers to the same structs and battle paths. | Run A1-A10 and B1-B11 after the merge and do the section 3.2 checklist; do not accept a merge based only on `git merge` exit status. |

## 7. Evidence and reproduction

```bash
# throwaway clone used for this plan (never push this ref)
cd /tmp/opencode/z-merge63
git clone -s --no-checkout /home/klisee/pokemon-reloaded-linux .sim
git -C .sim checkout main
# apply the order in section 2, then:
cd .sim/src/OpenGMK
cargo test --workspace --locked --offline
cargo test --locked --offline -p gm8emulator --lib
cargo test --locked --offline -p reloaded-cleanroom --lib
cargo test --locked --offline -p reloaded-online --test z063_flow
```

Simulation result: merge tip `2a1ac9b`; 662 tests passed in the workspace,
0 failed; the only unresolved release item is R1 (build string gate).
