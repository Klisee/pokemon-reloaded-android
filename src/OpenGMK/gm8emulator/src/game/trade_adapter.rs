//! F3R adapter for the clean-room trade codec and policy.
//!
//! Native `global.` names, field positions, selue mask extraction and party
//! read/write stay in this module. `trade_codec` has no game types.

use super::{trade_codec as codec, Game};
use crate::compat::f3r;
use crate::gml::{self, Value as GmlValue};
use crate::math::Real;

/// Minimal host surface used by capture/apply. `Game` is the production
/// implementation; tests use an in-memory store so validation and write order
/// can be exercised without an OpenGL window or a compiled game.
trait TradeHost {
    fn trade_global_value(&self, name: &[u8], index: u32) -> Option<GmlValue>;
    fn trade_global_text(&self, name: &[u8]) -> Option<Vec<u8>>;
    fn trade_instance_value(&self, object: i32, name: &[u8]) -> Option<GmlValue>;
    fn trade_set_global_value(&mut self, name: &[u8], index: u32, value: GmlValue);
    fn trade_global_num(&self, name: &[u8], index: u32) -> f64;
    fn trade_set_global_num(&mut self, name: &[u8], index: u32, value: f64);
}

impl TradeHost for Game {
    fn trade_global_value(&self, name: &[u8], index: u32) -> Option<GmlValue> {
        self.global_value(name, index)
    }

    fn trade_global_text(&self, name: &[u8]) -> Option<Vec<u8>> {
        self.global_text(name)
    }

    fn trade_instance_value(&self, object: i32, name: &[u8]) -> Option<GmlValue> {
        self.instance_value(object, name)
    }

    fn trade_set_global_value(&mut self, name: &[u8], index: u32, value: GmlValue) {
        self.set_global_value(name, index, value);
    }

    fn trade_global_num(&self, name: &[u8], index: u32) -> f64 {
        self.global_num(name, index)
    }

    fn trade_set_global_num(&mut self, name: &[u8], index: u32, value: f64) {
        self.set_global_num(name, index, value);
    }
}

pub const FIELD_COUNT: usize = 28;
pub const VALUE_COUNT: usize = 49;

pub const FIELDS: [&[u8]; FIELD_COUNT] = [
    b"pkmn1",
    b"nivel1",
    b"indexnivel",
    b"exp1",
    b"IV",
    b"objpoke1",
    b"pkmnsex1",
    b"indexpkmn",
    b"extraInfo",
    b"selue_pk1",
    b"PP_1",
    b"PS_1",
    b"STD1",
    b"MAX_PP_1",
    b"MAX_PS_1",
    b"XTRA_PS_1",
    b"atk1",
    b"def1",
    b"vel1",
    b"atk_sp1",
    b"def_sp1",
    b"prec1",
    b"num_attack1",
    b"tipoA1",
    b"tipoB1",
    b"tipoC1",
    b"volador1",
    b"fantasma1",
];

#[derive(Clone, Debug, PartialEq)]
pub enum TradeValue {
    Real(f64),
    Text(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TradeOffer {
    pub label: String,
    pub values: Vec<TradeValue>,
}

impl TradeOffer {
    fn to_codec(&self) -> Result<codec::Offer, String> {
        if self.values.len() != codec::FIELD_COUNT {
            return Err("el intercambio no tiene 49 campos".into())
        }
        let values: Vec<codec::Value> = self
            .values
            .iter()
            .map(|value| match value {
                TradeValue::Real(value) => codec::Value::Number(*value),
                TradeValue::Text(value) => codec::Value::Text(value.clone()),
            })
            .collect();
        codec::Offer::from_values(values).map_err(|error| error.code().to_string())
    }

    fn from_codec(message: codec::DecodedMessage) -> Self {
        let values: Vec<TradeValue> = message
            .offer
            .fields
            .iter()
            .map(|value| match value {
                codec::Value::Number(value) => TradeValue::Real(*value),
                codec::Value::Text(value) => TradeValue::Text(value.clone()),
            })
            .collect();
        Self {
            label: String::from_utf8_lossy(&message.label).into_owned(),
            values,
        }
    }
}

/// Policy gate. Clean-room validation runs structure, type, envelopes, normalization,
/// story/Master Ball gates, cross-field consistency and text rules in the order of
/// `trade-values.md` section 10. The mask is `None`: the domain check belongs to the
/// capture/apply path where the local mask is known.
pub fn validate_offer(offer: &TradeOffer) -> Result<(), String> {
    let offer = offer.to_codec()?;
    codec::validate_safety(&offer, None)
        .map(|_| ())
        .map_err(|error| error.code().to_string())
}

/// Packed fields only (no label). Kept for tests and rollback parity.
pub fn encode(offer: &TradeOffer) -> Result<Vec<u8>, String> {
    let offer = offer.to_codec()?;
    codec::encode(&offer).map_err(|error| error.code().to_string())
}

/// `label + 0x1e + packed fields`, exactly what `reloaded_online::send_peer_message`
/// expects for `PeerMessageKind::Offer`; the transport adds `OFFER\x1f`.
pub fn encode_message(offer: &TradeOffer) -> Result<Vec<u8>, String> {
    let label = offer.label.as_bytes().to_vec();
    let offer = offer.to_codec()?;
    codec::encode_message(&offer, &label).map_err(|error| error.code().to_string())
}

/// Peer payload (after the transport removed `OFFER\x1f`). Enforces the 64 KiB frame limit
/// and the strict field count of the clean-room decoder.
pub fn decode(payload: &[u8]) -> Result<TradeOffer, String> {
    codec::decode_message(payload)
        .map(TradeOffer::from_codec)
        .map_err(|error| error.code().to_string())
}

fn value_from_option(value: Option<GmlValue>, text_default: bool) -> TradeValue {
    match value {
        Some(GmlValue::Real(value)) => TradeValue::Real(value.into_inner()),
        Some(GmlValue::Str(value)) => TradeValue::Text(value.as_ref().to_vec()),
        None if text_default => TradeValue::Text(Vec::new()),
        None => TradeValue::Real(0.0),
    }
}

fn say_my_name<H: TradeHost + ?Sized>(game: &H) -> Vec<u8> {
    game.trade_global_text(f3r::global::APODO_NAME)
        .filter(|name| !name.is_empty())
        .or_else(|| game.trade_global_text(f3r::global::PROTANAME))
        .unwrap_or_default()
}

fn mask<H: TradeHost + ?Sized>(game: &H) -> Result<u64, String> {
    let base = match game.trade_instance_value(f3r::object::CONTROL, f3r::field::MASK1) {
        Some(GmlValue::Real(value)) => {
            let value = value.into_inner();
            if !value.is_finite()
                || value < 0.0
                || value.fract() != 0.0
                || value > codec::MAX_SAFE_INTEGER as f64
            {
                return Err("mascara selue invalida".into())
            }
            value as u64
        },
        _ => 0,
    };
    codec::selue_mask(base).map_err(|error| error.code().to_string())
}

pub fn selue_mask(game: &Game) -> Result<u64, String> {
    mask(game)
}

pub fn selue_transport(value: f64, mask: u64) -> Result<f64, String> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0
        || value > codec::MAX_SAFE_INTEGER as f64
    {
        return Err("selue fuera de rango".into())
    }
    codec::selue_transport(value as u64, mask)
        .map(|value| value as f64)
        .map_err(|error| error.code().to_string())
}

fn number(value: &TradeValue) -> Result<f64, String> {
    match value {
        TradeValue::Real(value) => Ok(*value),
        TradeValue::Text(_) => Err("campo numerico de intercambio invalido".into()),
    }
}

fn real(value: &TradeValue) -> f64 {
    match value {
        TradeValue::Real(value) => *value,
        TradeValue::Text(value) => std::str::from_utf8(value)
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0.0),
    }
}

/// Capture party slot `slot` into 49 values. Species fields are canonicalized with
/// `trade_codec::canonical_species` (mod 100000 above the threshold), selue fields 9 and
/// info 14 use the local mask, info 4 falls back to the local trainer name, and info 5 is
/// always text. The label is display-only.
pub fn capture(game: &Game, slot: u8) -> Result<TradeOffer, String> {
    capture_with(game, slot)
}

fn capture_with<H: TradeHost + ?Sized>(game: &H, slot: u8) -> Result<TradeOffer, String> {
    if usize::from(slot) >= f3r::format::PARTY_SIZE {
        return Err("slot de intercambio invalido".into())
    }
    let mask = mask(game)?;
    let mut values = Vec::with_capacity(VALUE_COUNT);
    for (index, name) in FIELDS.iter().enumerate() {
        let mut value = value_from_option(game.trade_global_value(name, u32::from(slot)), false);
        if index == 0 || index == 7 {
            if let TradeValue::Real(ref mut value) = value {
                *value = codec::canonical_species(*value)
                    .map_err(|error| error.code().to_string())? as f64;
            }
        }
        if index == 9 {
            if let TradeValue::Real(ref mut value) = value {
                *value = selue_transport(*value, mask)?;
            }
        }
        values.push(value);
    }
    for move_index in 1..=4u32 {
        values.push(value_from_option(
            game.trade_global_value(f3r::global::ATTACK1, u32::from(slot) * f3r::format::PARTY_ARRAY_STRIDE + move_index),
            false,
        ));
    }
    for info_index in 0..=16u32 {
        let mut value = value_from_option(
            game.trade_global_value(f3r::global::INFO, u32::from(slot) * f3r::format::PARTY_ARRAY_STRIDE + info_index),
            info_index == 4 || info_index == 5,
        );
        if info_index == 4 {
            let name = say_my_name(game);
            match &value {
                TradeValue::Text(text) if !text.is_empty() => {},
                _ => value = TradeValue::Text(name),
            }
        }
        if info_index == 5 && !matches!(value, TradeValue::Text(_)) {
            value = TradeValue::Text(Vec::new());
        }
        if info_index == 14 {
            if let TradeValue::Real(ref mut value) = value {
                *value = selue_transport(*value, mask)?;
            }
        }
        values.push(value);
    }
    let nickname = match &values[28 + 4 + 5] {
        TradeValue::Text(value) if !value.is_empty() => String::from_utf8_lossy(value).into_owned(),
        _ => String::new(),
    };
    let level = real(&values[1]);
    let label = if nickname.is_empty() {
        format!("Pokemon Nv. {level:.0}")
    } else {
        format!("{nickname} Nv. {level:.0}")
    };
    Ok(TradeOffer { label, values })
}

/// Apply a validated offer to party slot `slot`. Validates through the clean-room policy
/// first, then writes: canonical species in fields 0 and 7, inverse selue transport in
/// field 9 and info 14, OT cleared when it matches a local name form, nickname preserved.
/// All fallible work happens before the first `set_value`; the write loop itself cannot
/// fail. The caller (`trade_tick`) still wraps apply+save in `OnlineSnapshot` for the save
/// failure rollback required by TRADE-23.
pub fn apply(game: &mut Game, slot: u8, offer: &TradeOffer) -> Result<(), String> {
    apply_with(game, slot, offer)
}

fn apply_with<H: TradeHost + ?Sized>(
    game: &mut H,
    slot: u8,
    offer: &TradeOffer,
) -> Result<(), String> {
    if usize::from(slot) >= f3r::format::PARTY_SIZE || offer.values.len() != VALUE_COUNT {
        return Err("intercambio invalido".into())
    }
    let codec_offer = offer.to_codec()?;
    codec::validate_safety(&codec_offer, None).map_err(|error| error.code().to_string())?;
    let mask = mask(game)?;

    let species = codec::canonical_species(number(&offer.values[0])?)
        .map_err(|error| error.code().to_string())?;
    let index_species = codec::canonical_species(number(&offer.values[7])?)
        .map_err(|error| error.code().to_string())?;
    if species != index_species {
        return Err("E_SPECIES_MISMATCH".into())
    }

    let mut scalars: Vec<TradeValue> = offer.values[..28].to_vec();
    scalars[0] = TradeValue::Real(species as f64);
    scalars[7] = TradeValue::Real(index_species as f64);
    scalars[9] = TradeValue::Real(selue_transport(number(&scalars[9])?, mask)?);

    let mut infos: Vec<TradeValue> = offer.values[32..49].to_vec();
    let mut names = vec![
        say_my_name(game),
        game.trade_global_text(f3r::global::PROTANAME).unwrap_or_default(),
        game.trade_global_text(f3r::global::APODO_NAME).unwrap_or_default(),
    ];
    names.sort();
    names.dedup();
    for info_index in 0..=16u32 {
        let index = info_index as usize;
        let mut value = infos[index].clone();
        if info_index == 14 {
            value = TradeValue::Real(selue_transport(number(&value)?, mask)?);
        }
        if info_index == 4 {
            value = match value {
                TradeValue::Text(text)
                    if !text.is_empty() && !names.iter().any(|name| name == &text) =>
                {
                    TradeValue::Text(text)
                },
                _ => TradeValue::Text(Vec::new()),
            };
        }
        infos[index] = value;
    }

    for (index, name) in FIELDS.iter().enumerate() {
        set_value(game, name, u32::from(slot), scalars[index].clone());
    }
    for move_index in 1..=4u32 {
        set_value(
            game,
            f3r::global::ATTACK1,
            u32::from(slot) * f3r::format::PARTY_ARRAY_STRIDE + move_index,
            offer.values[28 + move_index as usize - 1].clone(),
        );
    }
    for info_index in 0..=16u32 {
        set_value(
            game,
            f3r::global::INFO,
            u32::from(slot) * f3r::format::PARTY_ARRAY_STRIDE + info_index,
            infos[info_index as usize].clone(),
        );
    }
    let count = (0..f3r::format::PARTY_SIZE as u8)
        .filter(|slot| game.trade_global_num(f3r::global::PKMN1, u32::from(*slot)) > 0.0)
        .count() as f64;
    if game.trade_global_num(f3r::global::ACTUAL1, 0) >= count {
        game.trade_set_global_num(f3r::global::ACTUAL1, 0, 0.0);
    }
    Ok(())
}

fn set_value<H: TradeHost + ?Sized>(game: &mut H, name: &[u8], index: u32, value: TradeValue) {
    let value = match value {
        TradeValue::Real(value) => GmlValue::Real(Real::from(value)),
        TradeValue::Text(value) => GmlValue::Str(gml::String::from(value)),
    };
    game.trade_set_global_value(name, index, value);
}

pub fn is_valid_slot(game: &Game, slot: u8) -> bool {
    is_valid_slot_with(game, slot)
}

fn is_valid_slot_with<H: TradeHost + ?Sized>(game: &H, slot: u8) -> bool {
    slot <= 5 && game.trade_global_num(f3r::global::PKMN1, u32::from(slot)) > 0.0
}

pub fn occupied_slots(game: &Game) -> Vec<u8> {
    (0..f3r::format::PARTY_SIZE as u8)
        .filter(|slot| is_valid_slot_with(game, *slot))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const GOLDEN_LABEL: &str = "Fixture Nv. 50";

    fn n(value: f64) -> TradeValue {
        TradeValue::Real(value)
    }

    fn t(value: &[u8]) -> TradeValue {
        TradeValue::Text(value.to_vec())
    }

    fn golden_offer() -> TradeOffer {
        TradeOffer {
            label: GOLDEN_LABEL.into(),
            values: vec![
                n(448.0),
                n(50.0),
                n(50.0),
                n(125_000.0),
                n(31.0),
                n(0.0),
                n(1.0),
                n(448.0),
                n(0.0),
                n(123_456.0),
                n(10.0),
                n(150.0),
                n(0.0),
                n(10.0),
                n(150.0),
                n(0.0),
                n(100.0),
                n(80.0),
                n(90.0),
                n(110.0),
                n(85.0),
                n(100.0),
                n(1.0),
                n(2.0),
                n(17.0),
                n(0.0),
                n(0.0),
                n(0.0),
                n(33.0),
                n(34.0),
                n(35.0),
                n(36.0),
                n(0.0),
                n(0.0),
                n(0.0),
                n(0.0),
                t(b"Trainer"),
                t(b"Fixture"),
                n(0.0),
                n(0.0),
                n(0.0),
                n(0.0),
                n(0.0),
                n(0.0),
                n(0.0),
                n(0.0),
                n(0.0),
                n(0.0),
                n(0.0),
            ],
        }
    }

    fn safety_ready_offer() -> TradeOffer {
        let mut offer = golden_offer();
        offer.values[21] = n(0.0);
        offer.values[22] = n(4.0);
        offer.values[33] = n(50.0);
        offer.values[46] = n(123_456.0);
        offer
    }

    fn offer_for_length(target: usize) -> TradeOffer {
        let mut offer = golden_offer();
        offer.label = "L".repeat(codec::MAX_LABEL_LEN);
        let current = encode_message(&offer).unwrap().len();
        let extra = target.saturating_sub(current);
        let text_len = match &offer.values[37] {
            TradeValue::Text(text) => text.len(),
            _ => 0,
        };
        offer.values[37] = TradeValue::Text(vec![b'a'; text_len + extra]);
        offer
    }

    fn replace(offer: &TradeOffer, index: usize, value: TradeValue) -> TradeOffer {
        let mut offer = offer.clone();
        offer.values[index] = value;
        offer
    }

    #[derive(Clone, Default)]
    struct FakeHost {
        globals: HashMap<(Vec<u8>, u32), GmlValue>,
        instances: HashMap<(i32, Vec<u8>), GmlValue>,
        writes: Vec<(Vec<u8>, u32)>,
    }

    impl FakeHost {
        fn with_mask(base: f64) -> Self {
            let mut host = Self::default();
            host.instance(3, f3r::field::MASK1, GmlValue::Real(Real::from(base)));
            host
        }

        fn put(&mut self, name: &[u8], index: u32, value: GmlValue) {
            self.globals.insert((name.to_vec(), index), value);
        }

        fn put_num(&mut self, name: &[u8], index: u32, value: f64) {
            self.put(name, index, GmlValue::Real(Real::from(value)));
        }

        fn put_text(&mut self, name: &[u8], value: &[u8]) {
            self.put(name, 0, GmlValue::Str(gml::String::from(value)));
        }

        fn instance(&mut self, object: i32, name: &[u8], value: GmlValue) {
            self.instances.insert((object, name.to_vec()), value);
        }

        fn clear_writes(&mut self) {
            self.writes.clear();
        }
    }

    impl TradeHost for FakeHost {
        fn trade_global_value(&self, name: &[u8], index: u32) -> Option<GmlValue> {
            self.globals.get(&(name.to_vec(), index)).cloned()
        }

        fn trade_global_text(&self, name: &[u8]) -> Option<Vec<u8>> {
            match self.trade_global_value(name, 0)? {
                GmlValue::Str(value) => Some(value.as_ref().to_vec()),
                GmlValue::Real(value) => Some(format!("{:.0}", value.into_inner()).into_bytes()),
            }
        }

        fn trade_instance_value(&self, object: i32, name: &[u8]) -> Option<GmlValue> {
            self.instances.get(&(object, name.to_vec())).cloned()
        }

        fn trade_set_global_value(&mut self, name: &[u8], index: u32, value: GmlValue) {
            self.writes.push((name.to_vec(), index));
            self.put(name, index, value);
        }

        fn trade_global_num(&self, name: &[u8], index: u32) -> f64 {
            match self.trade_global_value(name, index) {
                Some(GmlValue::Real(value)) => value.into_inner(),
                Some(GmlValue::Str(value)) => std::str::from_utf8(value.as_ref())
                    .ok()
                    .and_then(|text| text.trim().parse().ok())
                    .unwrap_or(0.0),
                None => 0.0,
            }
        }

        fn trade_set_global_num(&mut self, name: &[u8], index: u32, value: f64) {
            self.writes.push((name.to_vec(), index));
            self.put_num(name, index, value);
        }
    }

    #[test]
    fn offered_fixture_is_safety_ready() {
        let offer = safety_ready_offer();
        assert_eq!(validate_offer(&offer), Ok(()));
        assert_eq!(offer.values.len(), VALUE_COUNT);
    }

    #[test]
    fn offer_transport_uses_label_separator_not_envelope() {
        let offer = golden_offer();
        let payload = encode_message(&offer).unwrap();
        assert!(payload.starts_with(GOLDEN_LABEL.as_bytes()));
        assert!(!payload.starts_with(b"OFFER"));
        assert_eq!(payload[GOLDEN_LABEL.len()], codec::LABEL_SEPARATOR);
        assert_eq!(
            &payload[GOLDEN_LABEL.len() + 1..],
            encode(&offer).unwrap().as_slice()
        );

        let decoded = decode(&payload).unwrap();
        assert_eq!(decoded.label, GOLDEN_LABEL);
        assert_eq!(decoded.values, offer.values);

        let wire = [b"OFFER\x1f".as_slice(), payload.as_slice()].concat();
        let decoded = decode(&wire).unwrap();
        assert_eq!(decoded.label, GOLDEN_LABEL);
        assert_eq!(decoded.values, offer.values);
    }

    #[test]
    fn offer_transport_rejects_empty_and_malformed_payloads() {
        assert_eq!(decode(b""), Err("E_TERMINATOR".into()));
        assert_eq!(decode(b"OFFER\x1f"), Err("E_TERMINATOR".into()));
        assert_eq!(decode(b"Fixture\x1e"), Err("E_TERMINATOR".into()));

        let offer = golden_offer();
        let mut truncated = encode(&offer).unwrap();
        truncated.pop();
        assert_eq!(decode(&truncated), Err("E_TERMINATOR".into()));

        let mut extra_field = encode(&offer).unwrap();
        extra_field.extend_from_slice(b"r0.000000\x1d");
        assert_eq!(decode(&extra_field), Err("E_COUNT".into()));
    }

    #[test]
    fn offer_transport_accepts_exactly_64_kib_and_rejects_the_next_byte() {
        let max = codec::MAX_MESSAGE_LEN;
        let offer = offer_for_length(max);
        let payload = encode_message(&offer).unwrap();
        assert_eq!(payload.len(), max);
        assert!(decode(&payload).is_ok());

        let mut too_long = payload;
        too_long.push(b'A');
        assert_eq!(decode(&too_long), Err("E_FRAME_LIMIT".into()));

        let too_long_offer = offer_for_length(max + 1);
        assert_eq!(
            encode_message(&too_long_offer),
            Err("E_FRAME_LIMIT".into())
        );
    }

    #[test]
    fn label_limits_are_enforced_on_send_and_receive() {
        let max_label = vec![b'L'; codec::MAX_LABEL_LEN];
        let mut offer = golden_offer();
        offer.label = String::from_utf8(max_label).unwrap();
        assert!(encode_message(&offer).is_ok());

        offer.label.push('L');
        assert_eq!(encode_message(&offer), Err("E_LABEL_LIMIT".into()));

        offer.label = "A\u{1e}B".into();
        assert_eq!(encode_message(&offer), Err("E_LABEL_SEPARATOR".into()));

        let packed = encode(&golden_offer()).unwrap();
        let mut raw = vec![b'A'; codec::MAX_LABEL_LEN + 1];
        raw.push(codec::LABEL_SEPARATOR);
        raw.extend_from_slice(&packed);
        assert_eq!(decode(&raw), Err("E_LABEL_LIMIT".into()));
    }

    #[test]
    fn story_gate_covers_direct_wrapped_and_shiny_species() {
        for species in [
            150.0, 151.0, 249.0, 493.0, 517.0, 2046.0, 2282.0, 100_150.0, 200_150.0,
            1150.0, 3046.0, 101_150.0, 150_150.0,
        ] {
            let offer = replace(
                &replace(&safety_ready_offer(), 0, n(species)),
                7,
                n(species),
            );
            assert_eq!(validate_offer(&offer), Err("E_STORY".into()), "{species}");
        }

        let index_only = replace(&safety_ready_offer(), 7, n(150.0));
        assert_eq!(validate_offer(&index_only), Err("E_STORY".into()));

        for species in [1.0, 448.0, 100_001.0, 100_448.0] {
            let offer = replace(
                &replace(&safety_ready_offer(), 0, n(species)),
                7,
                n(species),
            );
            assert_eq!(validate_offer(&offer), Ok(()), "{species}");
        }
    }

    #[test]
    fn master_ball_item_and_capture_marker_are_rejected() {
        let held = replace(&safety_ready_offer(), 5, n(codec::MASTER_BALL_ITEM_ID as f64));
        assert_eq!(validate_offer(&held), Err("E_ITEM_MASTER".into()));

        let caught = replace(&safety_ready_offer(), 32, n(codec::MASTER_BALL_MARKER as f64));
        assert_eq!(validate_offer(&caught), Err("E_BALL_MASTER".into()));

        let both = replace(
            &replace(&safety_ready_offer(), 5, n(4.0)),
            32,
            n(3.0),
        );
        assert_eq!(validate_offer(&both), Err("E_ITEM_MASTER".into()));

        for item in [0.0, 3.0] {
            assert_eq!(
                validate_offer(&replace(&safety_ready_offer(), 5, n(item))),
                Ok(()),
                "{item}"
            );
        }
        for marker in [0.0, 2.0] {
            assert_eq!(
                validate_offer(&replace(&safety_ready_offer(), 32, n(marker))),
                Ok(()),
                "{marker}"
            );
        }
        assert_eq!(
            validate_offer(&replace(&safety_ready_offer(), 5, n(4.5))),
            Err("E_NUMBER".into())
        );
        assert_eq!(
            validate_offer(&replace(&safety_ready_offer(), 32, n(3.5))),
            Err("E_NUMBER".into())
        );
    }

    fn capture_host() -> FakeHost {
        let mut host = FakeHost::with_mask(9_321.0);
        host.put_num(b"pkmn1", 0, 100_448.0);
        host.put_num(b"nivel1", 0, 50.0);
        host.put_num(b"indexnivel", 0, 50.0);
        host.put_num(b"indexpkmn", 0, 100_448.0);
        host.put_num(b"selue_pk1", 0, 1_000_000.0);
        host.put_num(b"PP_1", 0, 10.0);
        host.put_num(b"PS_1", 0, 150.0);
        host.put_num(b"MAX_PP_1", 0, 10.0);
        host.put_num(b"MAX_PS_1", 0, 150.0);
        host.put_num(b"num_attack1", 0, 4.0);
        for move_index in 1..=4 {
            host.put_num(f3r::global::ATTACK1, move_index, move_index as f64);
        }
        host.put_text(f3r::global::APODO_NAME, b"Ash");
        host.put_text(f3r::global::PROTANAME, b"Gary");
        for info_index in 0..=16u32 {
            if info_index == 4 {
                host.put(f3r::global::INFO, info_index, GmlValue::Str(gml::String::from(b"".as_slice())));
            } else if info_index == 5 {
                host.put(f3r::global::INFO, info_index, GmlValue::Str(gml::String::from(b"000123".as_slice())));
            } else if info_index == 14 {
                host.put_num(f3r::global::INFO, info_index, 1_000_000.0);
            } else {
                host.put_num(f3r::global::INFO, info_index, info_index as f64);
            }
        }
        host
    }

    #[test]
    fn capture_canonicalizes_species_and_transports_selue() {
        let host = capture_host();
        let offer = capture_with(&host, 0).unwrap();
        assert_eq!(offer.values.len(), VALUE_COUNT);
        assert_eq!(offer.values[0], n(448.0));
        assert_eq!(offer.values[7], n(448.0));
        assert_eq!(offer.values[9], n(9_327_000_000.0));
        assert_eq!(offer.values[32 + 14], n(9_327_000_000.0));
        assert_eq!(offer.values[32 + 4], t(b"Ash"));
        assert_eq!(offer.values[32 + 5], t(b"000123"));
        assert_eq!(offer.label, "000123 Nv. 50");
        assert_eq!(offer.values[28..32], [n(1.0), n(2.0), n(3.0), n(4.0)]);
    }

    fn apply_host() -> FakeHost {
        let mut host = FakeHost::with_mask(9_321.0);
        host.put_text(f3r::global::APODO_NAME, b"Ash");
        host.put_text(f3r::global::PROTANAME, b"Gary");
        host.put_num(b"pkmn1", 0, 1.0);
        host.put_num(b"actual1", 0, 6.0);
        host
    }

    #[test]
    fn apply_canonicalizes_reverses_selue_and_clears_local_ot() {
        let mut host = apply_host();
        let mut offer = safety_ready_offer();
        offer.values[0] = n(100_448.0);
        offer.values[7] = n(100_448.0);
        offer.values[9] = n(9_327_000_000.0);
        offer.values[46] = n(9_327_000_000.0);
        offer.values[32 + 4] = t(b"Ash");
        offer.values[32 + 5] = t(b"Nick");
        offer.values[32 + 14] = n(9_327_000_000.0);

        apply_with(&mut host, 3, &offer).unwrap();

        assert_eq!(host.trade_global_num(b"pkmn1", 3), 448.0);
        assert_eq!(host.trade_global_num(b"indexpkmn", 3), 448.0);
        assert_eq!(host.trade_global_num(b"selue_pk1", 3), 1_000_000.0);
        assert_eq!(host.trade_global_num(f3r::global::INFO, 3 * f3r::format::PARTY_ARRAY_STRIDE + 14), 1_000_000.0);
        assert_eq!(
            host.trade_global_value(f3r::global::INFO, 3 * f3r::format::PARTY_ARRAY_STRIDE + 4),
            Some(GmlValue::Str(gml::String::from(b"".as_slice())))
        );
        assert_eq!(
            host.trade_global_value(f3r::global::INFO, 3 * f3r::format::PARTY_ARRAY_STRIDE + 5),
            Some(GmlValue::Str(gml::String::from(b"Nick".as_slice())))
        );
        assert_eq!(host.trade_global_num(b"actual1", 0), 0.0);
    }

    #[test]
    fn apply_writes_scalars_moves_info_then_repairs_actual_slot() {
        let mut host = apply_host();
        apply_with(&mut host, 2, &safety_ready_offer()).unwrap();

        let mut expected: Vec<(Vec<u8>, u32)> = FIELDS
            .iter()
            .map(|name| (name.to_vec(), 2))
            .collect();
        for move_index in 1..=4 {
            expected.push((f3r::global::ATTACK1.to_vec(), 2 * f3r::format::PARTY_ARRAY_STRIDE + move_index));
        }
        for info_index in 0..=16 {
            expected.push((f3r::global::INFO.to_vec(), 2 * f3r::format::PARTY_ARRAY_STRIDE + info_index));
        }
        expected.push((f3r::global::ACTUAL1.to_vec(), 0));

        assert_eq!(host.writes, expected);
    }

    #[test]
    fn apply_validation_failure_writes_nothing() {
        let mut host = apply_host();
        let before = host.globals.clone();

        let story = replace(
            &replace(&safety_ready_offer(), 0, n(150.0)),
            7,
            n(150.0),
        );
        assert_eq!(apply_with(&mut host, 0, &story), Err("E_STORY".into()));

        let mismatch = replace(&safety_ready_offer(), 46, n(0.0));
        assert_eq!(
            apply_with(&mut host, 0, &mismatch),
            Err("E_SELUE".into())
        );

        let item = replace(&safety_ready_offer(), 5, n(4.0));
        assert_eq!(
            apply_with(&mut host, 0, &item),
            Err("E_ITEM_MASTER".into())
        );

        assert!(host.writes.is_empty());
        assert_eq!(host.globals, before);
    }

    #[test]
    fn apply_failure_after_validation_also_writes_nothing() {
        let mut host = FakeHost::with_mask(-1.0);
        host.put_text(f3r::global::APODO_NAME, b"Ash");
        let before = host.globals.clone();

        assert_eq!(
            apply_with(&mut host, 0, &safety_ready_offer()),
            Err("mascara selue invalida".into())
        );
        assert!(host.writes.is_empty());
        assert_eq!(host.globals, before);
    }

    #[test]
    fn capture_encode_decode_apply_round_trip_preserves_party_values() {
        let captured = capture_with(&capture_host(), 0).unwrap();
        let payload = encode_message(&captured).unwrap();
        let decoded = decode(&payload).unwrap();

        let mut target = apply_host();
        apply_with(&mut target, 1, &decoded).unwrap();

        assert_eq!(target.trade_global_num(b"pkmn1", 1), 448.0);
        assert_eq!(target.trade_global_num(b"nivel1", 1), 50.0);
        assert_eq!(target.trade_global_num(b"selue_pk1", 1), 1_000_000.0);
        assert_eq!(target.trade_global_num(f3r::global::INFO, f3r::format::PARTY_ARRAY_STRIDE + 14), 1_000_000.0);
        assert_eq!(
            target.trade_global_value(f3r::global::ATTACK1, f3r::format::PARTY_ARRAY_STRIDE + 4),
            Some(GmlValue::Real(Real::from(4.0)))
        );
        assert_eq!(
            target.trade_global_value(f3r::global::INFO, f3r::format::PARTY_ARRAY_STRIDE + 5),
            Some(GmlValue::Str(gml::String::from(b"000123".as_slice())))
        );
    }
}
