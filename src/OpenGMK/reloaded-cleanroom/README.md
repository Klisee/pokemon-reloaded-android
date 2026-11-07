# reloaded-cleanroom

Independent, spec-derived modules for Reloaded OpenGMK. This crate does not
depend on F3R GML, the game executable, game data, or the host emulator.

## Modules
- `team`: `.TEAM` codec, validation, versions, checksum.
- `trade`: 49-field offer codec, safety validation, story/Master Ball policy.
- `world`: WP/WDEL/WPOS/WLIST parsing and sprite mapping.
- `save`: generic atomic save interception, backups, rollback.
- `ports`: declaration-only core/adapter interfaces.

## Provenance
The modules were written from the functional specifications in
`cleanroom/spec/` and black-box vectors, not from the game's GML or assets.
The crate is isolated so the host can adopt it behind a compatibility adapter.

## Tests
`cargo test -p reloaded-cleanroom --offline` currently runs 98 tests.
