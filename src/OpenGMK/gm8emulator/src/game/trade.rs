//! Trade session state machine and F3R adapter facade.
//!
//! Wire codec and validation policy live in `trade_codec` (clean-room, no game types).
//! F3R field/global mapping lives in `trade_adapter` (adapter boundary).

#[allow(unused_imports)]
pub use super::trade_adapter::{
    apply, capture, decode, encode, encode_message, is_valid_slot, occupied_slots,
    validate_offer, TradeOffer, TradeValue, FIELD_COUNT, VALUE_COUNT,
};

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
