# Zibelan / ROZ protocol — static reverse-engineering notes

Status: research note, not a normative wire contract. It records what can be
established statically about the Zibelan/ROZ line protocol used by Reloaded
Online, and separates observed behaviour from legacy/compatibility behaviour.

Scope: handshake commands, version/build metadata, `WELCOME`/`WOK`/`PEER_LEFT`
semantics, and the trigger for the welcome/connected notice. No endpoint was
contacted while producing this note.

## 1. Method and evidence

Static sources only:

- The capture-derived clean-room specification `cleanroom/spec/roz-protocol.md`
  (draft, based on a passive TCP capture: 11 connections, 6,634 log lines,
  `WELCOME` ids 7432–7466, peer ids up to 7470).
- The compatibility requirements in `cleanroom/spec/compatibility.md` and the
  acceptance criteria in `cleanroom/spec/acceptance.md`.
- Installed APK 0.6.7 (`lib/arm64-v8a/libgm8emulator.so` and the dex string
  pool), inspected with `strings`/`grep` only.
- The stock reference client `ROZ.dll` (PE32, i386), inspected statically. Its
  command literals and parser state transitions were recovered with `objdump`.
- The clean-room codec and host client in this repository:
  `src/OpenGMK/reloaded-cleanroom/src/roz.rs`,
  `src/OpenGMK/online/src/roz.rs`, and
  `src/OpenGMK/gm8emulator/src/reloaded_online.rs`.
- Existing local logs and verification notes. No raw Zibelan packet capture is
  present in this worktree; the usable protocol transcript is the one summarized
  in `roz-protocol.md`.

The clean-room capture spec is the primary evidence for the modern public
server. `ROZ.dll` and the compatibility profile describe the older/stock path.
Where they disagree, this note records the disagreement instead of silently
mixing them.

## 2. Transport and framing

- Plain TCP; no TLS, compression, length prefix, or JSON envelope.
- One command per line, terminated by LF (`0x0A`). A CR immediately before LF is
  tolerated and stripped.
- Command names are uppercase ASCII. Arguments are positional and separated by
  single spaces. The final text field of `WP`/`START`/`WSAY` consumes the rest of
  the line.
- No escaping. Outgoing CR/LF in a name or text field must be rejected or
  replaced with spaces so that one call cannot become two protocol lines.
- The capture's longest line was 1,202 bytes (`MSG`). The codec's local safety
  cap is 64 KiB, with a compatibility floor of 8 KiB.
- `MSG` carries lowercase hex in both directions; incoming hex should accept
  either case.

## 3. Handshake

### 3.1 World/direct Zibelan mode (modern server)

The client sends its join burst immediately after connecting, in this order:

| Order | Line | Meaning | Encoding |
|---|---|---|---|
| 1 | `WJOIN <name>` | announce the local display name | name is the rest of the line; CR/LF sanitized |
| 2 | `WTID <prota_ID>` | announce the trainer id | decimal digits, matching `string_digits(string(global.prota_ID))` |
| 3 | `WTSPR <sprD>` | announce the player sprite | optional; sent only when `sprD` exists |
| 4 | `WVER <version>` | announce protocol version | canonical decimal `u32`; current `NetInit` value `8` |
| 5 | `WBUILD <build>` | announce client build | canonical decimal `u64`; current `NetInit` value `20261109` |

Current opening transcript (older captures show the three-line `WVER 6` /
`WBUILD 20261103` form without `WTID`/`WTSPR`):

```text
C: WJOIN Klisee
C: WTID 00123
C: WTSPR 456
C: WVER 8
C: WBUILD 20261109
S: WELCOME 7432
S: WOK 7432
C: WSYNC
```

The server greets with `WELCOME <id>` on accept, then answers the join with
`WOK <id>` using the same id. `WSYNC` requests the initial world snapshot. The
client does not choose its numeric id, and there is no password, token, or
resume state.

The installed 0.6.7 native library contains the older contiguous `.rodata`
bytes `WVER 6WBUILD 20261103`. The current `NetInit` script declares
`NET_VERSION="8"` and `NET_BUILD=20261109`, and `NetWorldConnect` writes
`WJOIN`, `WTID`, optional `WTSPR`, `WVER` and `WBUILD` in that order. The Rust
host uses `RozClient::world_handshake` and the clean-room codec exposes the same
sequence in `encode_world_handshake`
(`src/OpenGMK/reloaded-cleanroom/src/roz.rs`).

Expectations that follow from the evidence:

- `WVER` is a small protocol version; `8` is the current `NetInit` value.
- `WBUILD` is an opaque decimal build stamp; `20261109` looks like a date but
  must not be interpreted as game logic unless a separate requirement says so.
- `WTID` is always sent in world mode and `WTSPR` only when the game has a
  `sprD` global.
- The version/build fields are sent before the server is considered joined.
  The current source comments say the server uses them to reject mixed client
  versions before a battle starts.
- The stock `ROZ.dll` contains no `WVER`/`WBUILD` literals; its `net_join`
  builds only `HELLO`/`JOIN`, and `net_line` is a raw escape hatch. The
  version/build metadata therefore belongs to the modern/captured profile, not
  to the oldest stock path.

### 3.2 Lobby/stock mode

The stock reference client exposes a separate lobby handshake through
`net_join(room, name)`:

| Order | Line | Meaning |
|---|---|---|
| 1 | `HELLO <name>` | local display name |
| 2 | `JOIN <room>` | lobby/room name |

It does not send `WVER`/`WBUILD` in this path. The server answers `WAIT` while
the room is waiting for a peer, then `START <side> <peer-name>` once two players
are paired. `START` is the authoritative acceptance; a client must not require a
preceding `WANS`.

`ROZ.dll` constructs `HELLO ` and `JOIN ` internally and stores the first
numeric token of `START` as the session side, with the rest of the line as the
peer name (recovered from the `net_join` and parser code). This is the stock
profile; it is not what the modern capture shows for `WOK`.

### 3.3 Status values

Both the stock client and the Rust host expose the same numeric status model:

| Value | Meaning |
|---|---|
| 0 | disconnected |
| 1 | connecting / handshake pending |
| 2 | waiting (`WAIT`) |
| 3 | started (`START`) |
| 4 | peer left (`PEER_LEFT`) |
| 5 | full (`FULL` room or `WFULL` world) |
| 6 | world joined (`WOK`) |

`FULL` affects a room and `WFULL` affects the world; both map to status 5 but
must stay distinguishable in the event model. `WELCOME` does not change the
numeric status by itself.

## 4. `WELCOME` semantics

Wire form: `WELCOME <id>`, exactly one decimal field. The value is an unsigned
64-bit connection identity with no advertised width limit.

Evidence for identity, not a protocol banner:

- The server sends `WELCOME` on TCP accept, even for a connection that sent no
  protocol handshake at all (an HTTP `GET /` still received it).
- In normal sessions the following `WOK` carries the same number.
- Captured values were 7432–7466 and increased across connections, including the
  non-protocol connection. Peer ids in world traffic extended to 7470.
- Reconnecting produces a fresh `WELCOME` id; there is no session resume.

Therefore, for the modern public Zibelan server, `WELCOME` is the session
identity/greeting. It is **not** a protocol-version banner.

This conflicts with older stock/compatibility wording that describes
`WELCOME 3011` as a version banner and not a player id. That wording appears to
describe the legacy profile or an obsolete assumption; the primary capture
evidence says identity. The current client unfortunately keeps both meanings:
it sets `local_id` from `WELCOME` and also derives a displayed “protocol” number
from the same value
(`src/OpenGMK/online/src/roz.rs:924-926,1005-1008`;
`src/OpenGMK/gm8emulator/src/reloaded_online.rs:1364-1366,1640-1647`). That is a
known defect: the protocol version is `WVER 8`, not the id.

Recommended handling:

- Accept the first canonical `WELCOME <nonzero u64>` on a connection.
- Set the local identity once; use it only for self-suppression.
- Do not delete/evict a remote peer merely because its id equals a `WELCOME` or
  `WOK` value; identity assignment and world deletion are separate operations.
- Ignore duplicate `WELCOME` lines within the same connection epoch. A different
  id on a second `WELCOME` is a protocol violation, not a new session.
- Drop malformed lines (missing, non-numeric, negative, extra fields) instead of
  presenting `id #0`. The current legacy fallback maps them to 0
  (`src/OpenGMK/online/src/roz.rs:1005-1008`), which can evict a real peer 0 and
  produce a bogus welcome.
- A reconnect is a new connection epoch with a fresh id; reset dedupe state.

## 5. `WOK` semantics

Wire form: `WOK <id>`, exactly one decimal field.

Observed/primary meaning:

- It repeats the `WELCOME` id and is a generic acknowledgement.
- It is also sent after `WEND`, not only after the initial `WJOIN`/`WVER`/
  `WBUILD` handshake.
- It is not a unique join response and is not a barrier for `WP`/`WPOS` traffic.
- The id is an unsigned 64-bit value in the modern profile.

Recommended handling:

- Use the first `WOK` whose id matches the established `WELCOME` id to promote
  the session to “world joined”/status 6.
- Never use `WOK` to originate identity, a welcome notice, or a peer eviction.
- Never treat a post-`WEND` `WOK` as a new join.
- Keep the full `u64` for identity comparisons. Only convert to `i32` when a
  profile explicitly defines that value as a battle side.

Legacy/stock caveat: `ROZ.dll` has no `WELCOME` parser and treats the numeric
argument of `WOK` as the session side (`net_status` 6, `net_side` from the same
value). The compatibility profile likewise describes `WOK <side>`. That is a
real profile difference, not evidence that the modern server sends the side in
`WOK`. The current host conflates the two (`WorldJoined { side, id }`) and
rejects ids above `i32::MAX` (`src/OpenGMK/online/src/roz.rs:932-946,1027-1036`),
so a modern four-digit id is silently treated as a side. Treat `WOK` as ack/id
in the modern profile and as side only in the legacy profile.

## 6. `PEER_LEFT` semantics

Wire form: `PEER_LEFT` with no arguments.

- It reports that the interaction peer has disappeared.
- It clears the peer name and moves the session to status 4 (peer left).
- It is not `WDEL`: it does not remove the peer from the general world stream.
  `WDEL <id>` is the world-visibility removal; the same id can reappear after a
  room change.
- It is not a TCP close by itself. The world stream may continue, but any open
  trade/battle with that peer must be failed closed; do not wait for a `WDEL` to
  clean up interaction state.
- The stock client maps it to status 4 and clears the peer; `net_input_get`
  returns its “not in START” sentinel (-2) so a blocked battle-input wait wakes
  up.
- The current host sets `peer_left`, cancels battle/trade, and reports
  “Zibelan: el rival salio”, but does not remove the remote world record
  (`src/OpenGMK/gm8emulator/src/reloaded_online.rs:1411-1415`). That matches the
  spec: leave world removal to `WDEL`/teardown.

## 7. Welcome/connected notice trigger

The correct trigger for the once-per-session welcome or “connected to the
online world” notice is the **first canonical `WELCOME <id>`** on a connection.

Rules established from the capture and the connection-notice review:

- Trigger on `WELCOME`, not on `WOK`, not on `WJOIN`, and not on TCP connect.
- Require a valid, non-zero unsigned id. Do not show `id #0` for malformed lines.
- Emit exactly one notice per connection epoch. Later `WELCOME` lines and all
  later `WOK` lines are silent for notice/chat/status purposes.
- A matching first `WOK` may promote the persistent status to “world joined” and
  may set the profile’s side, but must not replay the notice.
- A post-`WEND` `WOK`, a mismatched `WOK`, or a `WOK` before `WELCOME` must not
  create a notice or identity.
- Key dedupe on `(server, connection epoch, id)`, not on the rendered text. Reset
  it on stop, failure, and reconnect.
- A reconnect is a new epoch and shows a fresh notice only after its own
  `WELCOME`; it must not replay the old one.
- Keep the notice and the protocol version separate: show the connection id from
  `WELCOME` and, if desired, the protocol version from `WVER` (8), never the id
  as the version.

Why the legacy client can appear to trigger on `WOK`: the stock client does not
parse `WELCOME`; it shows its world-success dialog when `net_status` reaches 6
(`WOK`), and its lobby-success dialog when status reaches 3 (`START`). That is a
legacy UI/status trigger, not the modern protocol’s identity trigger. A modern
implementation should prefer `WELCOME` for identity/welcome and use `WOK` only
for the joined acknowledgement.

Current implementation gap: `RozEvent::Welcome` calls `apply_local_id` and
`announce_welcome`; `WorldJoined` also calls `apply_local_id` and announces a
welcome whenever the id changes
(`src/OpenGMK/gm8emulator/src/reloaded_online.rs:1364-1396`). That means a
`WOK` can fabricate a welcome, a duplicate `WELCOME` can evict a peer, and the
notice dedupe is keyed by text rather than by session. The status line
“Zibelan conectado; esperando WOK” is only progress text; it is not the notice.

## 8. Current implementation vs the evidence

| Topic | Evidence | Current code | Assessment |
|---|---|---|---|
| World handshake | `WJOIN`, `WTID`, optional `WTSPR`, `WVER 8`, `WBUILD 20261109`, then `WSYNC` | same order via `RozClient::world_handshake` and typed codec commands | wire-compatible; bounded wait for status 6/5 |
| Lobby handshake | `HELLO <name>`, `JOIN <room>` | `hello`, `join` | matches stock profile; absent from capture |
| `WELCOME` | session identity `u64`, first one only | `Welcome { version, id }`, applies every event, version derived from id | semantics/notification need correction |
| `WOK` | generic ack/id, also after `WEND` | parsed as `i32` side plus optional `u64` id; announces welcome on change | conflates ack/id/side; rejects large ids |
| `PEER_LEFT` | interaction peer gone, status 4, no world deletion | clears peer, sets flag, fails battle/trade, keeps remote | largely aligned; UI/cleanup gaps remain |
| Welcome notice | first valid `WELCOME`, once per epoch | first `WELCOME` plus `WOK` fallback, text-keyed dedupe | trigger not strict enough |
| Malformed lines | discard/ignore, no identity change | legacy fallback maps malformed `WELCOME` to 0 and `WOK -1` to side -1 | must not be treated as canonical |

The canonical parser in `reloaded-cleanroom/src/roz.rs` already models the
evidence correctly: `WELCOME <u64>` and `WOK <u64>` at lines 496–503,
`PEER_LEFT` at 521–523, and the three-line handshake at 426–431. Existing tests
cover full-width ids, malformed forms, CRLF, and the handshake order. No parser
change or new parser test was needed for this note; the open work is host
semantics, not grammar.

## 9. Uncertainties and open questions

1. `WELCOME` meaning is inconsistent in the written specs: the capture-derived
   spec says identity; the compatibility/acceptance wording says banner. This
   note follows the capture for the modern server. A future capture should pin
   the stock/banner variant if it is still supported.
2. `WJOIN` is described as a display name by the capture and the current source,
   but as a world name in the compatibility table. For the modern profile, treat
   it as the self-declared display name until a capture shows otherwise.
3. It is not proven whether the server enforces `WVER`/`WBUILD` for ordinary
   world play or only before battles. The current source comments say they are
   used to reject mixed client versions before a battle.
4. The exact relationship between `WOK`-as-ack/id (modern) and `WOK`-as-side
   (stock `ROZ.dll`) is unresolved. Do not assume both are true on one endpoint.
5. The maximum id width, whether id 0 is ever valid, and whether ids can be
   reused across a server restart are not established.
6. `PEER_LEFT` versus `WDEL` timing is not fully captured: the spec says
   `PEER_LEFT` is interaction-level and `WDEL` is world-level, but a server may
   send both for a disconnect.
7. The notice copy and whether it is a modal dialog, a status line, or chat is a
   UI decision; the protocol only requires that the first valid `WELCOME` is the
   identity event. The exact-once dedupe key must be session-scoped.
8. The modern spec marks `WAIT`, `FULL`, `WFULL`, `WANS`, and `WCANCEL` as
   provisional; the stock client does implement `WAIT`/`FULL`/`WFULL`, but the
   live server was not captured emitting all of them.
9. No raw packet capture is present in this worktree, so all live-server claims
   are second-hand from the admitted clean-room capture summary.

## 10. Safe parser-test policy

- Do not add tests that connect to `64.177.48.215` or any live endpoint.
- Parser tests must use in-memory bytes or a local mock socket only.
- Canonical grammar (`WELCOME`/`WOK`/`PEER_LEFT`, handshake order, CRLF, line
  limits) is already covered in `reloaded-cleanroom/src/roz.rs` and
  `src/OpenGMK/online/src/roz.rs`. Add tests there only for a concrete parser
  change; do not encode live-server assumptions in tests before the wire
  semantics above are resolved.
- A future host test for the welcome trigger should assert: first valid
  `WELCOME` produces exactly one notice; duplicate `WELCOME`/`WOK`, `WOK` before
  `WELCOME`, mismatched `WOK`, and malformed `WELCOME` produce none; a reconnect
  produces one new notice with the new id.
