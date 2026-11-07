//! F3R adapter for the clean-room trade codec and policy.
//!
//! Native `global.` names, field positions, selue mask extraction and party
//! read/write stay in this module. `trade_codec` has no game types.

use super::{trade_codec as codec, Game};
use crate::compat::f3r;
use crate::gml::{self, Value as GmlValue};
use crate::math::Real;

pub const FIELD_COUNT: usize = 28;
pub const VALUE_COUNT: usize = 49;

pub const FIELDS: [&[u8]; FIELD_COUNT] = [
    f3r::global::PKMN1,
    f3r::global::NIVEL1,
    f3r::global::INDEXNIVEL,
    f3r::global::EXP1,
    f3r::global::IV,
    f3r::global::OBJPOKE1,
    f3r::global::PKMNSEX1,
    f3r::global::INDEXPKMN,
    f3r::global::EXTRA_INFO,
    f3r::global::SELUE_PK1,
    f3r::global::PP_1,
    f3r::global::PS_1,
    f3r::global::STD1,
    f3r::global::MAX_PP_1,
    f3r::global::MAX_PS_1,
    f3r::global::XTRA_PS_1,
    f3r::global::ATK1,
    f3r::global::DEF1,
    f3r::global::VEL1,
    f3r::global::ATK_SP1,
    f3r::global::DEF_SP1,
    f3r::global::PREC1,
    f3r::global::NUM_ATTACK1,
    f3r::global::TIPO_A1,
    f3r::global::TIPO_B1,
    f3r::global::TIPO_C1,
    f3r::global::VOLADOR1,
    f3r::global::FANTASMA1,
];

#[derive(Clone, Debug, PartialEq)]
pub enum TradeValue {
    Real(f64),
    Text(Vec<u8>),
}

#[derive(Clone, Debug)]
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

fn say_my_name(game: &Game) -> Vec<u8> {
    game.global_text(f3r::global::APODO_NAME)
        .filter(|name| !name.is_empty())
        .or_else(|| game.global_text(f3r::global::PROTANAME))
        .unwrap_or_default()
}

fn mask(game: &Game) -> Result<u64, String> {
    let base = match game.instance_value(f3r::object::CONTROL, f3r::field::MASK1) {
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
    if usize::from(slot) >= f3r::format::PARTY_SIZE {
        return Err("slot de intercambio invalido".into())
    }
    let mask = mask(game)?;
    let mut values = Vec::with_capacity(VALUE_COUNT);
    for (index, name) in FIELDS.iter().enumerate() {
        let mut value = value_from_option(game.global_value(name, u32::from(slot)), false);
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
    for move_index in 1..=f3r::format::MOVE_COUNT as u32 {
        values.push(value_from_option(
            game.global_value(f3r::global::ATTACK1, u32::from(slot) * f3r::format::PARTY_ARRAY_STRIDE + move_index),
            false,
        ));
    }
    for info_index in 0..f3r::format::INFO_FIELDS as u32 {
        let mut value = value_from_option(
            game.global_value(f3r::global::INFO, u32::from(slot) * f3r::format::PARTY_ARRAY_STRIDE + info_index),
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
        game.global_text(f3r::global::PROTANAME).unwrap_or_default(),
        game.global_text(f3r::global::APODO_NAME).unwrap_or_default(),
    ];
    names.sort();
    names.dedup();
    for info_index in 0..f3r::format::INFO_FIELDS as u32 {
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
    for move_index in 1..=f3r::format::MOVE_COUNT as u32 {
        set_value(
            game,
            f3r::global::ATTACK1,
            u32::from(slot) * f3r::format::PARTY_ARRAY_STRIDE + move_index,
            offer.values[28 + move_index as usize - 1].clone(),
        );
    }
    for info_index in 0..f3r::format::INFO_FIELDS as u32 {
        set_value(
            game,
            f3r::global::INFO,
            u32::from(slot) * f3r::format::PARTY_ARRAY_STRIDE + info_index,
            infos[info_index as usize].clone(),
        );
    }
    let count = (0..f3r::format::PARTY_SIZE).filter(|slot| game.global_num(f3r::global::PKMN1, *slot as u32) > 0.0).count() as f64;
    if game.global_num(f3r::global::ACTUAL1, 0) >= count {
        game.set_global_num(f3r::global::ACTUAL1, 0, 0.0);
    }
    Ok(())
}

fn set_value(game: &mut Game, name: &[u8], index: u32, value: TradeValue) {
    let value = match value {
        TradeValue::Real(value) => GmlValue::Real(Real::from(value)),
        TradeValue::Text(value) => GmlValue::Str(gml::String::from(value)),
    };
    game.set_global_value(name, index, value);
}

pub fn is_valid_slot(game: &Game, slot: u8) -> bool {
    usize::from(slot) < f3r::format::PARTY_SIZE && game.global_num(f3r::global::PKMN1, u32::from(slot)) > 0.0
}

pub fn occupied_slots(game: &Game) -> Vec<u8> {
    (0..f3r::format::PARTY_SIZE as u8).filter(|slot| is_valid_slot(game, *slot)).collect()
}
