//! Clean-room implementation of the Zibelan trade payload and session state.
//!
//! This module deliberately does not touch battle input/checksum state. The wire format is the
//! 49-field `NetPokemonPack` format from the reference client: 28 party scalars, four moves and
//! 17 `info` fields, each prefixed with `s`/`r` and terminated by `0x1d`.

use crate::gml::{self, Value};
use crate::math::Real;
use super::Game;

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

#[derive(Clone, Debug)]
pub struct TradeOffer {
    pub label: String,
    pub values: Vec<TradeValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradePhase {
    Pick,
    WaitingOffer,
    Confirm,
    WaitingOk,
}

#[derive(Clone, Debug)]
pub struct TradeSession {
    pub peer: u64,
    pub name: String,
    pub phase: TradePhase,
    pub slot: Option<u8>,
    pub local: Option<TradeOffer>,
    pub remote: Option<TradeOffer>,
    pub selection: usize,
    pub message: String,
}

impl TradeSession {
    pub fn new(peer: u64, name: String) -> Self {
        Self {
            peer,
            name,
            phase: TradePhase::Pick,
            slot: None,
            local: None,
            remote: None,
            selection: 0,
            message: String::new(),
        }
    }
}

fn value_from_option(value: Option<Value>, text_default: bool) -> TradeValue {
    match value {
        Some(Value::Real(value)) => TradeValue::Real(value.into_inner()),
        Some(Value::Str(value)) => TradeValue::Text(value.as_ref().to_vec()),
        None if text_default => TradeValue::Text(Vec::new()),
        None => TradeValue::Real(0.0),
    }
}

fn say_my_name(game: &Game) -> Vec<u8> {
    game.global_text(b"apodoName")
        .filter(|name| !name.is_empty())
        .or_else(|| game.global_text(b"protaname"))
        .unwrap_or_default()
}

pub fn selue_mask(game: &Game) -> f64 {
    let base = game
        .instance_value(3, b"mask1")
        .and_then(|value| match value {
            Value::Real(value) => Some(value.into_inner()),
            _ => None,
        })
        .unwrap_or(0.0);
    let mask = base + 7.0;
    if mask < 5000.0 {
        mask + 5000.0
    } else {
        mask
    }
}

pub fn selue_transport(value: f64, mask: f64) -> f64 {
    if value < 1_000_000.0 {
        return value
    }
    let high = (value / 1_000_000.0).floor();
    let low = value - high * 1_000_000.0;
    (mask - high) * 1_000_000.0 + low
}

fn text(value: &TradeValue) -> Vec<u8> {
    match value {
        TradeValue::Text(value) => value.clone(),
        TradeValue::Real(value) => format!("{value}").into_bytes(),
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

pub fn capture(game: &Game, slot: u8) -> Result<TradeOffer, String> {
    if slot > 5 {
        return Err("slot de intercambio invalido".into())
    }
    let mask = selue_mask(game);
    let mut values = Vec::with_capacity(VALUE_COUNT);
    for (index, name) in FIELDS.iter().enumerate() {
        let mut value = value_from_option(game.global_value(name, u32::from(slot)), false);
        if index == 0 {
            if let TradeValue::Real(ref mut value) = value {
                if *value > 100_000.0 {
                    *value %= 100_000.0;
                }
            }
        }
        if index == 9 {
            if let TradeValue::Real(ref mut value) = value {
                *value = selue_transport(*value, mask);
            }
        }
        values.push(value);
    }
    for move_index in 1..=4u32 {
        values.push(value_from_option(
            game.global_value(b"attack1", u32::from(slot) * 32_000 + move_index),
            false,
        ));
    }
    for info_index in 0..=16u32 {
        let mut value = value_from_option(
            game.global_value(b"info", u32::from(slot) * 32_000 + info_index),
            info_index == 4 || info_index == 5,
        );
        if info_index == 4 {
            let name = say_my_name(game);
            match &value {
                TradeValue::Text(text) if !text.is_empty() => {},
                _ => value = TradeValue::Text(name),
            }
        }
        if info_index == 14 {
            if let TradeValue::Real(ref mut value) = value {
                *value = selue_transport(*value, mask);
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

pub fn encode(offer: &TradeOffer) -> Result<Vec<u8>, String> {
    if offer.values.len() != VALUE_COUNT {
        return Err("el intercambio no tiene 49 campos".into())
    }
    let mut output = Vec::new();
    for value in &offer.values {
        match value {
            TradeValue::Real(value) => {
                if !value.is_finite() {
                    return Err("valor no finito en el intercambio".into())
                }
                output.extend_from_slice(format!("r{value:.6}").as_bytes());
            },
            TradeValue::Text(value) => {
                if value.contains(&0x1d) {
                    return Err("texto con separador de intercambio".into())
                }
                output.push(b's');
                output.extend_from_slice(value);
            },
        }
        output.push(0x1d);
    }
    Ok(output)
}

pub fn decode(payload: &[u8]) -> Result<TradeOffer, String> {
    let (label, packed) = match payload.iter().position(|byte| *byte == 0x1e) {
        Some(position) => (
            String::from_utf8_lossy(&payload[..position]).into_owned(),
            &payload[position + 1..],
        ),
        None => (String::new(), payload),
    };
    if !packed.ends_with(&[0x1d]) {
        return Err("el intercambio no termina en separador".into())
    }
    let mut parts: Vec<&[u8]> = packed.split(|byte| *byte == 0x1d).collect();
    if parts.last().is_some_and(|part| part.is_empty()) {
        parts.pop();
    }
    if parts.len() != VALUE_COUNT {
        return Err(format!("el intercambio tiene {} campos, se esperaban 49", parts.len()))
    }
    let mut values = Vec::with_capacity(VALUE_COUNT);
    for part in parts {
        let Some((prefix, body)) = part.split_first() else {
            return Err("campo de intercambio vacio".into())
        };
        match *prefix {
            b's' => values.push(TradeValue::Text(body.to_vec())),
            b'r' => {
                let value = std::str::from_utf8(body)
                    .map_err(|_| "numero de intercambio no UTF-8")?
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| "numero de intercambio invalido")?;
                if !value.is_finite() {
                    return Err("numero de intercambio no finito".into())
                }
                values.push(TradeValue::Real(value));
            },
            _ => return Err("prefijo de intercambio invalido".into()),
        }
    }
    if !matches!(values[0], TradeValue::Real(value) if value > 0.0) {
        return Err("especie de intercambio invalida".into())
    }
    if !matches!(values[1], TradeValue::Real(value) if value >= 1.0) {
        return Err("nivel de intercambio invalido".into())
    }
    Ok(TradeOffer { label, values })
}

pub fn apply(game: &mut Game, slot: u8, offer: &TradeOffer) -> Result<(), String> {
    if slot > 5 || offer.values.len() != VALUE_COUNT {
        return Err("intercambio invalido".into())
    }
    let mask = selue_mask(game);
    for (index, name) in FIELDS.iter().enumerate() {
        let mut value = offer.values[index].clone();
        if index == 9 {
            if let TradeValue::Real(ref mut value) = value {
                *value = selue_transport(*value, mask);
            }
        }
        set_value(game, name, u32::from(slot), value);
    }
    for move_index in 1..=4u32 {
        set_value(
            game,
            b"attack1",
            u32::from(slot) * 32_000 + move_index,
            offer.values[28 + move_index as usize - 1].clone(),
        );
    }
    for info_index in 0..=16u32 {
        let mut value = offer.values[32 + info_index as usize].clone();
        if info_index == 14 {
            if let TradeValue::Real(ref mut value) = value {
                *value = selue_transport(*value, mask);
            }
        }
        if info_index == 4 {
            let own = say_my_name(game);
            value = match value {
                TradeValue::Text(text)
                    if !text.is_empty()
                        && text != own
                        && text != game.global_text(b"protaname").unwrap_or_default()
                        && text != game.global_text(b"apodoName").unwrap_or_default() =>
                {
                    TradeValue::Text(text)
                },
                _ => TradeValue::Text(Vec::new()),
            };
        }
        set_value(game, b"info", u32::from(slot) * 32_000 + info_index, value);
    }
    let count = (0..=5).filter(|slot| game.global_num("pkmn1", *slot) > 0.0).count() as f64;
    if game.global_num("actual1", 0) >= count {
        game.set_global_num("actual1", 0, 0.0);
    }
    Ok(())
}

fn set_value(game: &mut Game, name: &[u8], index: u32, value: TradeValue) {
    let value = match value {
        TradeValue::Real(value) => Value::Real(Real::from(value)),
        TradeValue::Text(value) => Value::Str(gml::String::from(value)),
    };
    game.set_global_value(name, index, value);
}

pub fn is_valid_slot(game: &Game, slot: u8) -> bool {
    slot <= 5 && game.global_num("pkmn1", u32::from(slot)) > 0.0
}

pub fn occupied_slots(game: &Game) -> Vec<u8> {
    (0..=5).filter(|slot| is_valid_slot(game, *slot)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selue_transport_round_trips_below_mask() {
        let mask = 9328.0;
        assert_eq!(selue_transport(0.0, mask), 0.0);
        assert_eq!(selue_transport(999_999.0, mask), 999_999.0);
        assert_eq!(selue_transport(1_000_000.0, mask), 9_327_000_000.0);
        assert_eq!(selue_transport(selue_transport(1_000_000.0, mask), mask), 1_000_000.0);
    }

    #[test]
    fn encode_uses_49_terminators() {
        let values = (0..VALUE_COUNT)
            .map(|index| if index < 2 { TradeValue::Real(1.0) } else { TradeValue::Text(Vec::new()) })
            .collect();
        let offer = TradeOffer { label: "x".into(), values };
        let encoded = encode(&offer).expect("encode");
        assert_eq!(encoded.iter().filter(|byte| **byte == 0x1d).count(), VALUE_COUNT);
        assert!(encoded.ends_with(&[0x1d]));
    }

    #[test]
    fn decode_rejects_wrong_count() {
        assert!(decode(b"").is_err());
        let values = (0..VALUE_COUNT)
            .map(|index| if index < 2 { TradeValue::Real(1.0) } else { TradeValue::Real(0.0) })
            .collect();
        let offer = TradeOffer { label: "x".into(), values };
        let encoded = encode(&offer).expect("encode");
        assert!(decode(&encoded[..encoded.len() - 1]).is_err());
    }
}
