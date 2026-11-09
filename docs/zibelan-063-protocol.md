# Zibelan 0.6.3 online protocol — static GML notes

Status: static research note. It describes the online protocol as implemented by
the extracted GML scripts in `/tmp/z063/net_scripts.gml` (49 scripts). The
artifact label `z063` is used as the profile name; the GML itself does not state
a version string. No live server was contacted and no packet capture was used.

The transport implementation is not in this GML. The scripts call into
`ROZ.dll` through the `net_*` external functions, so this note can establish
application payloads, ordering, field indices and state transitions, but not
TCP framing, line endings, or the DLL's internal parser. Where the evidence ends,
that is stated explicitly.

## 1. Evidence map

Primary evidence, all in `net_scripts.gml`:

| Area | Scripts |
|---|---|
| World connect / reconnect | `NetWorldConnect`, `NetWorldReconnectStep`, `NetWorldReconnectStart`, `NetWorldLost`, `NetWorldAutoReconnect` |
| World status / maintenance | `NetWorldStep`, `NetWorldSessionEnd`, `NetWorldReadAll`, `NetWorldInbox`, `NetWorldMessage` |
| Challenge/session entry | `NetWorldIncoming`, `NetWorldAsk`, `NetWorldTalk`, `NetWorldSession`, `NetPeerWait` |
| Battle negotiation | `NetFight`, `NetSendTeam`, `NetConfigString`, `NetConfigApply` |
| Frame/input loop | `NetStartBattle`, `NetBattleBegin`, `NetFrameBegin`, `NetBattleMessages`, `NetBattleEnd` |
| Timing | `NetPingPeer`, `NetRttSample`, `NetDelayFrames`, `NetDelayAdapt` |
| Integrity/diagnostics | `NetChecksum`, `NetChecksumSend`, `NetDesync`, `NetDesyncReport` |
| Failure handling | `NetConnectionLost`, `NetPeerFail` |

Companion evidence used only for global defaults: `/tmp/z063/scripts.gml`
contains the initializer that assigns `global.NET_VERSION` and
`global.NET_BUILD`. The net-script extraction itself only reads those globals.
The distinction matters for the version/build claim in section 10.

Field indices below are zero-based, matching the `global.NET_FIELD[n]` indexing
used by `NetConfigApply` and the other parsers.

## 2. Framing conventions visible in the GML

The scripts use four different payload layouts, so a single generic splitter is
not safe:

- **Line commands sent by the GML**, such as `WJOIN`, `WVER`, `WBUILD`,
  `WSYNC`, `WEND`, `WANS` and `WCANCEL`, are plain text lines. Arguments are
  normally separated by spaces.
- **Unit-separated commands** use `chr(31)` (US, U+001F) between the command
  name and the body. Examples: `TEAM`, `WCFG`, `SUM`, `BYE`, `PING`, `PONG`,
  `NO`, `WP`, `WDEL`.
- **Record/field separators** inside a body:
  - `chr(30)` (RS, U+001E) separates fields in `TEAM` and `SUM`.
  - `chr(29)` (GS, U+001D) separates fields in `WCFG` and in `WLIST` entries.
  - `BYE` unusually uses a space between its two values, not RS.
  - `WDESYNC` uses spaces only.
- **File-bearing messages** (`TEAM`) use `net_send_file`: the command and
  metadata are the message prefix, and the rest of the payload is the raw
  `.TEAM` file. `NetWriteFile` writes that remainder on the peer.

The GML does not show a length prefix, escaping, quoting, compression, or TLS.
Text fields are sometimes sanitized locally (`NetCleanText` strips control
characters and caps length; `NetWorldSay` strips characters below 32 and caps at
150), but the `TEAM`/`WCFG` payloads themselves have no general escape mechanism.
Line termination is owned by `ROZ.dll` and is not evidenced here.

## 3. World handshake

### 3.1 Endpoint selection

`NetWorldConnect` reads `data/text/mundo.txt`:

- line 1: server address (default `127.0.0.1`);
- line 2: port (default `5000`).

If the file is absent it is created with those defaults. `net_connect` is called
with the address and port. A return value of 0 aborts world mode with an offline
message.

### 3.2 Outbound sequence

Immediately after a successful `net_connect`, `NetWorldConnect` sends these
lines, in this order:

| Order | Line | Value source | Meaning |
|---|---|---|---|
| 1 | `WJOIN <name>` | `NetCleanText(SayMyName(), 16)` | self-declared display name, control-stripped and capped at 16 characters |
| 2 | `WTID <digits>` | `string_digits(string(global.prota_ID))` | numeric protagonist/trainer id, digits only |
| 3 | `WTSPR <sprD>` | `string(global.sprD)` | optional; sent only when `global.sprD` exists |
| 4 | `WVER <version>` | `global.NET_VERSION` | protocol version string |
| 5 | `WBUILD <build>` | `string(global.NET_BUILD)` | decimal build stamp |

`NetWorldReconnectStep` repeats exactly the same five sends after reconnecting.
There is no password, token, session key, or resume state in the GML handshake.
Identity is self-declared.

`WTID` and the optional `WTSPR` are additional relative to the older
capture-derived note in `docs/zibelan-protocol-re.md`, which records only
`WJOIN` + `WVER` + `WBUILD`. The `WVER`/`WBUILD` fields appear in both, but with
different values in this build (section 10).

### 3.3 Acceptance and timeout

`NetWorldConnect` polls `net_status`:

- while status is 1, it calls `io_handle`, sleeps 20 ms and retries;
- the poll is capped at 250 iterations (approximately 5 seconds);
- only status 6 is accepted as joined;
- status 5 produces the "world is full" dialog;
- any other non-6 status disconnects and falls back to offline play.

The GML does not parse a `WELCOME`, `WOK` or equivalent handshake response. Its
only acceptance signal is the external status becoming 6. The DLL may consume
such lines before exposing status, but that is not visible in this evidence.

The initial world snapshot request is not part of `NetWorldConnect`.
`NetWorldStep` sends `WSYNC` whenever the current room differs from
`global.WORLD_ROOM`, including the first world tick after joining and after a
room change.

### 3.4 Reconnect

`NetWorldLost` disconnects, clears world state and either starts automatic
reconnect (when `data/text/mundo_reconectar.txt` begins with `1`) or asks the
player. `NetWorldReconnectStep`:

- retries the socket connection with exponential backoff
  `min(60000, 3000 * 2^min(N, 5))` ms;
- repeats the five-line handshake;
- waits up to 6 seconds for status 6 while status is 1;
- on any other result, disconnects and schedules another retry;
- on success, marks the world online, clears the world map and shows the
  "reconnected" note.

## 4. Status model

`net_status` is an opaque external value. The GML only shows how selected values
are treated. No status constant table exists in `net_scripts.gml`.

| Value | Evidence in the GML | Interpretation |
|---|---|---|
| 0 | not referenced | not established here; the older stock model calls it disconnected |
| 1 | connect loop; reconnect waits while 1 until deadline | connecting / handshake pending |
| 2 | not referenced | not established here; do not import `WAIT` semantics from another profile |
| 3 | required by `NetPeerWait` and `NetPingPeer`; accepted by challenge waits; tolerated by `NetWorldStep` | paired session / peer interaction started |
| 4 | treated as "peer no longer waiting" by `NetWorldIncoming`/`NetWorldAsk`; tolerated by `NetWorldStep` | peer left or interaction no longer available |
| 5 | `NetWorldConnect` shows "world is full" | world full |
| 6 | sole success value in `NetWorldConnect`; normal in-world value in `NetWorldStep` | world joined / ready |

Relevant transitions:

- Connect: 1 → 6 on success; 5 is full; anything else is a failed connect.
- In-world: `NetWorldStep` calls `NetWorldLost` only when the status is not 6,
  3 or 4. Therefore 3 and 4 are tolerated transient states in the world.
- Challenge wait: status 3 ends the wait and starts the session; status 6 or 4
  means the other player stopped waiting.
- Peer wait during battle setup: only status 3 continues; any other status is
  reported as `#LEFT`.
- Teardown: `NetWorldSessionEnd` sends `WEND` and resets `global.NET_SIDE` to 0.

The numeric subset used (1, 3, 4, 5, 6) is consistent with the older stock
model (connecting, started, peer left, full, world joined). That consistency is
corroboration, not proof that all values have the same meaning in every profile.

## 5. `TEAM` negotiation: seed and delay

`NetFight` runs after a challenge is accepted. Side 1 is the host, side 2 the
guest (the number comes from `net_side`). Both export their local team file.

Side 1 generates the battle seed and sends `TEAM` with `net_send_file`. The
payload layout is:

| Field | Value | Meaning |
|---|---|---|
| command | `TEAM` + US | message name |
| 0 | seed | `floor(random(1000000000)) + 1`, i.e. 1 through 1,000,000,000 |
| 1 | fondo | `global.numfondobattle` |
| 2 | modo | `global.modeBattleBack` |
| 3 | sala | `BattleBackground(room, false)` |
| 4 | delay | `NetDelayFrames()` |
| tail | raw `.TEAM` file | team data, opaque in this extraction |

Every metadata field is terminated by RS. The fifth RS ends the metadata; what
follows is the file body. The receiver (`NetPeerWait("TEAM", ...)`) reads five
RS-terminated values into `global.NETW_VAL[0..4]`; whatever remains is written
to `online_J<side>_rival.TEAM`. The sender appends a trailing RS after the delay,
so the file body starts immediately after that separator.

Field handling on the guest:

- field 0 becomes the seed (the host's value is authoritative);
- field 4 is clamped with `median(2, value, 10)`, giving a delay of 2–10
  frames;
- fields 1–3 become the background, battle mode and battle-background room.

### 5.1 Initial delay measurement

Before `TEAM` is sent, `NetPingPeer` measures round-trip time:

- every 200 ms, for up to 1.5 seconds, it sends `PING` + US + the local
  millisecond timestamp;
- the peer answers `PONG` + US + the same timestamp;
- `NetRttSample` accepts values in the 0–1500 ms range and keeps the last four
  samples in a ring.

`NetDelayFrames` then takes the maximum of the four samples (or 2 if none was
valid), converts it to frames, and computes:

`delay = median(2, ceil((rtt/2 + 15) / frame_ms) + 1, 10)`

where `frame_ms = 1000 / max(room_speed, 1)`. The result is therefore always
2–10 frames. The host puts it in `TEAM` field 4; the guest applies the same
clamp to the received value. `NetPeerWait` also answers `PING` with `PONG` while
waiting for `TEAM` or `WCFG`.

Failure handling:

- if fewer than five fields are present, the guest sends `NO` + US and reports
  that the two clients use different game versions;
- if the file cannot be written or the teams cannot be loaded/validated, the
  guest sends `NO` + US and aborts;
- `NetPeerWait` also returns `#NO` when it receives `NO`, and `#LEFT`/`#TIME`
  on status loss or timeout.

There is no explicit version number in `TEAM`. The only version-mismatch signal
is a malformed/short metadata prefix. The `.TEAM` file format itself is not
defined in `net_scripts.gml`.

## 6. `WCFG` negotiation

After the `TEAM` exchange succeeds, side 1 builds a configuration string from
the current battle globals and sends it as `WCFG` + US + payload. Side 2 waits
for `WCFG` with `NetPeerWait` and applies it with `NetConfigApply`. The host's
configuration is authoritative; there is no guest counter-offer and no explicit
acknowledgement.

The payload is 37 zero-based fields separated by GS (`chr(29)`):

| Index | Field | Index | Field |
|---|---|---|---|
| 0 | `NUM_POKEMON1` | 1 | `NUM_POKEMON2` |
| 2 | `DIFICULTAD1` | 3 | `DIFICULTAD2` |
| 4 | `CAMBIOS1` | 5 | `CAMBIOS2` |
| 6 | `NUM_OBJ1` | 7 | `NUM_OBJ2` |
| 8 | `OBJ_ASG1` | 9 | `OBJ_ASG2` |
| 10 | `CONT_POKE1` | 11 | `CONT_POKE2` |
| 12 | `VEL_ATAQUE1` | 13 | `VEL_ATAQUE2` |
| 14 | `REST_PP1` | 15 | `REST_PP2` |
| 16 | `SHOW_TYPE` | 17 | `TIME_BATTLE` |
| 18 | `BTTBACK_INDEX` | 19 | `MUSIC_INDEX` |
| 20 | `SUBITA` | 21 | `BATTLE_MODE` |
| 22 | `DAY` | 23 | `global.shownumber` |
| 24 | `global.showbattleevents` | 25 | `global.showpanelcombate` |
| 26 | `global.shownumberPS` | 27 | `global.modeBattleBack` |
| 28 | `global.WorldClima` | 29 | `global.clima` |
| 30 | day/night hour | 31 | day/night minute |
| 32 | `REGLAS_J1` | 33 | `REGLAS_J2` |
| 34 | `OPTIONS_BT` | 35 | `MUSIC_NAME` |
| 36 | `txt_clima` | | |

Details that matter for a parser:

- `NetConfigString` emits exactly these 37 fields in this order.
- If `obj_DayNightControl` does not exist, fields 30 and 31 are the literal
  defaults `8` and `30`.
- `NetConfigApply` requires at least 36 GS separators (i.e. 37 fields); a
  shorter payload is silently ignored.
- Fields 0–31 are read as numbers; fields 32–36 are kept as strings.
- A change in field 18 (`BTTBACK_INDEX`) or field 27 (`modeBattleBack`) triggers
  `MultiplayerSetBackground`.
- The parser uses the shared `global.NET_FIELD` array with a 40-entry loop, so
  extra fields beyond 36 can be present but are not consumed.
- There is no escaping. A GS character inside `MUSIC_NAME` or `txt_clima` would
  shift or truncate later fields. The last field is the only one without a
  trailing separator.
- `WCFG` carries no version tag and no checksum of its own. Its correctness is
  only checked indirectly by the frame-state checksum, if a mismatch affects the
  checksummed state.

After `TEAM` and `WCFG`, both sides load the two team files and call
`NetStartBattle(seed)`. There is no `GO`/`START` line in this world-battle path;
the start is implicit once team loading succeeds. A player can still abort with
`NO`.

## 7. Battle input channel (`IN`)

The GML has no literal `IN` command. Per-frame input uses the external input
channel:

- `net_input_send(frame, word)`;
- `net_input_get(frame, timeout_ms)`;
- `net_set_epoch(epoch)`.

In this note that channel is called **IN** because it is the input path; its
actual DLL-side wire name is not visible.

### 7.1 Sampling

`NetBattleBegin` installs 22 key codes:

- bits/indices 0–10: up, down, left, right, attacks 1–4, switch,
  object assignment, Mega Evolution;
- indices 11–21: the same eleven controls for the other player.

`NetSampleLocal` samples only indices 0–10 and returns an 11-bit mask. If the
game window does not have focus (`net_focus` returns 0), it returns 0 rather
than a stall. There is no analog input.

### 7.2 Scheduling

`NET_DELAY` is the negotiated delay in frames (section 5). At the beginning of
`NetBattleBegin`, each side (except spectator side 3) pre-sends zero inputs for
frames 1 through `NET_DELAY`. `NET_LASTSENT` is set to `NET_DELAY`.

On each local frame `f` in `NetFrameBegin`:

- `t = f + NET_DELAY`;
- if `t > NET_LASTSENT`, the local input word is stored in
  `NET_MINE[t mod 256]`, sent with `net_input_send(t, word)`, and
  `NET_LASTSENT` becomes `t`.

The input ring has 256 slots. With the 2–10 frame delay range, wrap collisions
are not expected in normal play.

If the previous peer fetch took 25 ms or more, `NET_STALL` is set and bit 15 is
ORed into the next local input word. This stall flag is consumed by
`NetDelayAdapt` and then cleared.

### 7.3 Waiting and mapping

`NetFrameBegin` fetches the peer's word for frame `f` with
`net_input_get(f, 20)`. While it returns -1, the loop:

- calls `NetBattleMessages` every 10 iterations;
- shows "waiting for rival" after 25 iterations;
- allows Escape to leave;
- declares connection loss at 3000 iterations (`peer = -2`);
- records a stall if the wait lasted at least 25 ms.

Side mapping is fixed:

| Side | `m1` (11 bits) | `m2` (11 bits) |
|---|---|---|
| 1 (host) | local `NET_MINE[f mod 256]` | peer word |
| 2 (guest) | peer word | local `NET_MINE[f mod 256]` |
| 3 (spectator) | low 11 bits of peer word | bits 11–21 of peer word |

For sides 1 and 2, the local word uses bits 0–10 for the eleven controls and bit
15 for the stall flag. For spectator side 3, the peer word packs both 11-bit
halves and no local input or checksum is sent.

### 7.4 Delay adaptation

`NetDelayAdapt` runs on every frame for non-spectators and uses the stall bit in
either input half:

- every 60 frames, if at least 6 frames in the interval carried a stall flag,
  the delay increases by one frame, capped at 10;
- if at most 1 frame carried a stall flag, a counter advances; after 5
  consecutive clean intervals the delay decreases by one frame, but not below
  the initial measured delay (`NET_DMIN`);
- otherwise the clean-interval counter resets.

When the delay increases, the client fills the newly covered future frames with
zero inputs and advances `NET_LASTSENT`; this preserves a continuous input
stream. Each side adapts its own send delay from the stall bits it observes, so
the two send-ahead depths can diverge over time. That affects buffering and
latency, not frame numbering: both sides still exchange input by logical frame
number.

Input word layout in the 22-bit concatenated form:

| Bit range | Meaning |
|---|---|
| 0–10 | player 1 controls |
| 11–21 | player 2 controls |

At the end of each frame, `NetFrameBegin` reseeds the local random generator
with `(epoch mod 65536) * 16384 + (frame mod 16384)`. Combined with the shared
seed, this makes per-frame RNG state a function of `(epoch, frame)`.

## 8. `SUM`, checksums and desync

### 8.1 Checksum

`NetChecksum` builds a state string with `NetStateText()` and computes a rolling
checksum:

`c = (c * 31 + character_code) mod 1000003`, starting from `c = 0`.

`NetStateText` itself is outside `net_scripts.gml`. Companion evidence shows it
includes the count/sum of non-persistent object indices, active Pokémon
positions, active slots, the six party HP values (scaled by 10), and the active
PP values. A different `NetStateText` definition would change the checksum
domain, so this note does not treat the exact string as part of the stable wire
contract.

### 8.2 Cadence and slots

Every 30 frames, when the local side is not spectator 3, `NetFrameBegin` calls
`NetChecksumSend(f)`:

- slot `k = (f div 30) mod 64`;
- store local frame `f`, checksum and state text in `NET_SUMF[k]`,
  `NET_SUMV[k]`, `NET_SUMT[k]`;
- send `SUM` + US + frame + RS + checksum.

The 64-slot window covers 64 checksummed blocks (1920 frames) before overwrite.

### 8.3 Comparison rules

There are two symmetric checks:

- On send, if the peer has stored the same frame in the same slot
  (`NET_PSUMF[k] == f`) and its checksum differs, call `NetDesync(f)`.
- On receive, store the peer frame/checksum in `NET_PSUMF[k]`/`NET_PSUMV[k]`;
  if the local slot contains the same frame and the checksums differ, call
  `NetDesync(f)`.

A checksum is only compared when both sides' frame numbers are exactly equal
for that slot. Stale, overwritten or out-of-order slots are ignored, not treated
as errors.

### 8.4 Desync response

`NetDesync` fires at most once per battle:

- sets `NET_DESYNC` and appends ` - DESINCRONIZADO` to the stored caption
  (applied immediately only when not waiting and not in the Escape path);
- writes a log line if logging is enabled;
- calls `NetDesyncReport(frame)`.

`NetDesyncReport` does nothing for spectator side 3. It builds a diagnostic
string containing:

- the mismatching frame, the local current frame and the current delay;
- the local state summary for that frame, if the slot still matches;
- a histogram of non-persistent object names (up to 200 entries);
- room name, climate values, event/panel/number toggles, background values;
- the full `WCFG` string with GS replaced by commas, when `MULTIPLAYER` exists;
- the working directory and `version <NET_BUILD>`.

Control characters 0–31 are replaced by spaces and the report is capped at
30,000 characters. It is sent as a plain-space line:

`WDESYNC <epoch> <frame> <side> <report>`

Note that `WDESYNC` does not use US as the command/body separator, unlike
`SUM` and `BYE`. No parser or server-side handling for `WDESYNC` is present in
this extraction. There is no resynchronisation or rollback: detection is
diagnostic only.

## 9. `BYE`

`NetBattleEnd` sends `BYE` when the session is online:

`BYE` + US + `<epoch>` + space + `<frame>`

- the epoch is the battle seed, set in `NetBattleBegin` via `net_set_epoch`;
- the frame is the local `NET_FRAME` at the time the battle ends;
- the message is sent only when `NET_ON` is true; `NetLeaveBattle` calls
  `NetBattleEnd`, so normal exits and Escape paths send it.

Receiving side (`NetBattleMessages`):

- split the body at the first space;
- parse the first part as epoch and the rest as frame;
- if the epoch equals `NET_EPOCH`, store the frame in `NET_BYE_FRAME`;
- if the epoch differs, ignore the message as stale.

`NetFrameBegin` checks the stored frame before doing work: when
`NET_BYE_FRAME >= 0` and the local frame is greater than it, the client leaves
the battle. Thus the peer's declared final frame is allowed to finish; the local
client exits at the first frame after it.

`NetConnectionLost` also checks a pending `NET_BYE_FRAME` before falling back to
the generic "connection lost" text. `BYE` has no reason code; the local text is
chosen from the local side (spectator versus player) and the current state.

A hard socket loss may prevent the BYE from being delivered. In that case the
input timeout (`net_input_get` returning -1 until the 3000-iteration limit) is
the fallback path to `NetConnectionLost`.

## 10. Version and build

The handshake sends:

| Command | Global | Value in this artifact | Notes |
|---|---|---|---|
| `WVER` | `global.NET_VERSION` | `"8"` | string, sent verbatim |
| `WBUILD` | `global.NET_BUILD` | `20261109` | number, sent via `string()` |

The values are not assigned in `net_scripts.gml`; they are read there and
assigned in the companion `scripts.gml` global initializer. Treat them as
build-specific constants, not as values derived from the world or the user's
save.

`NET_BUILD` is also what `NetDesyncReport` prints as `version`. `NET_VERSION` is
not included in the desync report. There is no server version response parsed
by the GML; acceptance is only the status transition to 6, and version
compatibility is otherwise reduced to the `TEAM` metadata shape check.

Compatibility implication: this build advertises `WVER 8` / `WBUILD 20261109`,
whereas the older capture-derived note in `docs/zibelan-protocol-re.md` records
`WVER 6` / `WBUILD 20261103` and only three client lines. These are different
profiles or different builds. Do not assume that a server accepting the older
handshake will accept this one, or vice versa. In particular, this GML sends
`WTID` and optional `WTSPR` before `WVER`, and it does not wait for `WELCOME`
or `WOK` itself.

## 11. Uncertainties and open questions

1. **DLL-owned framing.** The GML calls `net_line`, `net_send`,
   `net_send_file`, `net_input_send` and `net_input_get`, but the DLL-side wire
   representation, line endings, and whether the input channel is a separate
   packet type are not in the extraction. The name `IN` is descriptive, not an
   observed literal.
2. **`WELCOME`/`WOK`.** They are absent from the GML handshake and from the
   world parser. The external library may consume them before exposing status
   6, but this evidence cannot confirm or deny their presence on the wire.
3. **Status values 0 and 2.** They are never used in `net_scripts.gml`. Their
   usual stock meanings (disconnected, waiting) come from other evidence, not
   from this file.
4. **Version/build provenance.** `NET_VERSION="8"` and `NET_BUILD=20261109`
   are from the companion global initializer, not from `net_scripts.gml` itself.
   The artifact label `z063` is not a version field in the GML.
5. **`TEAM` file format.** The `.TEAM` body is opaque here. The five metadata
   fields and the trailing separator are established, but the file's internal
   encoding, chunking, and any checksum are not.
6. **`TEAM` metadata sufficiency.** A peer with a different build but the same
   five-field shape would not be rejected by this check. The GML only detects a
   short/malformed prefix.
7. **`WCFG` field count and escaping.** The producer emits 37 fields and the
   consumer requires at least 36 separators, but the shared `NET_FIELD` array
   allows up to 40. Behaviour with extra fields or with a GS inside the final
   text fields is not defined here.
8. **Config authority.** The host sends `WCFG` and the guest applies it without
   an acknowledgement. A dropped or partially parsed `WCFG` is not explicitly
   detected; it can surface later as a checksum mismatch.
9. **`NetStateText` domain.** The exact checksummed state string is outside the
   extraction. The checksum algorithm and cadence are clear, but reproducing
   the checksum requires the companion function and the same object/state
   ordering.
10. **Desync coverage.** The 30-frame cadence and 64-slot window mean a
    transient divergence that does not affect a sampled frame can be missed.
    Reports are best-effort diagnostics, not a recovery protocol.
11. **`BYE` delivery.** Sending is conditional on `NET_ON`, and a lost socket
    can prevent delivery. The input-timeout path is the only fallback evidenced
    here.
12. **Spectator path.** Side 3 branches are visible in `NetFrameBegin`,
    `NetBattleMessages` and `NetDesyncReport`, but `NetSpectate` itself is not in
    the extraction, so spectator setup, delay assignment and seed source remain
    unknown.
13. **Server behaviour.** Nothing in this GML shows how the server or the DLL
    validates `WVER`, `WBUILD`, `TEAM` or `WCFG`, or how a `WDESYNC` report is
    handled. No live endpoint was contacted.

## 12. Parser/test cautions

- Use static fixtures only; do not add tests that connect to a live Zibelan
  endpoint.
- Do not use a single splitter for all commands. `TEAM`/`WCFG`/`SUM`/`BYE`
  select US first; `WCFG` then uses GS and `TEAM`/`SUM` use RS. `BYE` and
  `WDESYNC` use spaces inside the body.
- Preserve the trailing separator after the fifth `TEAM` metadata field; it is
  the boundary between metadata and the raw team file.
- Treat fields as zero-based and keep the 37-field `WCFG` order.
- Compare checksums only for equal frame numbers in the same 64-slot window;
  do not flag an unmatched slot as a desync.
- Gate `BYE` handling on the battle epoch; a stale epoch must be ignored.
