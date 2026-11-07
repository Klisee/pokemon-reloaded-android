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
    pub remote_ok: bool,
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
            remote_ok: false,
            selection: 0,
            message: String::new(),
        }
    }
}

/// Result of the transactional apply/save sequence used by `trade_tick`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TradeCommit {
    Committed,
    ApplyFailed(String),
    SaveFailed(String),
}

/// Run `apply`, then `save` only if apply succeeded. `restore` runs exactly once
/// after either failure, before the error is returned to the caller.
pub(crate) fn commit_transaction<S, A, B, R>(
    state: &mut S,
    apply: A,
    save: B,
    restore: R,
) -> TradeCommit
where
    A: FnOnce(&mut S) -> Result<(), String>,
    B: FnOnce(&mut S) -> Result<(), String>,
    R: FnOnce(&mut S),
{
    match apply(state) {
        Ok(()) => match save(state) {
            Ok(()) => TradeCommit::Committed,
            Err(error) => {
                restore(state);
                TradeCommit::SaveFailed(error)
            },
        },
        Err(error) => {
            restore(state);
            TradeCommit::ApplyFailed(error)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(label: &str) -> TradeOffer {
        TradeOffer {
            label: label.into(),
            values: vec![TradeValue::Real(0.0); VALUE_COUNT],
        }
    }

    #[test]
    fn session_binds_peer_and_name_on_creation() {
        let session = TradeSession::new(7, "Rival".into());
        assert_eq!(session.peer, 7);
        assert_eq!(session.name, "Rival");
        assert_eq!(session.phase, TradePhase::Pick);
        assert_eq!(session.slot, None);
        assert!(session.local.is_none());
        assert!(session.remote.is_none());
        assert!(!session.remote_ok);
        assert_eq!(session.selection, 0);
        assert!(session.message.is_empty());
    }

    #[test]
    fn sessions_do_not_share_offers_or_selection() {
        let mut first = TradeSession::new(1, "Uno".into());
        let second = TradeSession::new(2, "Dos".into());
        first.local = Some(offer("local"));
        first.remote = Some(offer("remote"));
        first.slot = Some(4);
        first.selection = 2;
        first.phase = TradePhase::Confirm;

        assert_eq!(second.peer, 2);
        assert_eq!(second.phase, TradePhase::Pick);
        assert_eq!(second.slot, None);
        assert!(second.local.is_none());
        assert!(second.remote.is_none());
        assert_eq!(second.selection, 0);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum AckOutcome {
        Wait,
        Complete,
        Incomplete,
    }

    // Mirrors the OK branches in `Game::trade_tick` and `Game::trade_input`.
    // Those methods need game assets and a window, so the ordering vectors drive
    // the same session transitions through this small test model.
    fn receive_remote_ok(session: &mut TradeSession) -> AckOutcome {
        session.remote_ok = true;
        if session.phase == TradePhase::WaitingOk {
            if session.slot.is_some() && session.remote.is_some() {
                AckOutcome::Complete
            } else {
                AckOutcome::Incomplete
            }
        } else {
            if session.phase == TradePhase::Confirm {
                session.message = "El rival acepto; confirma tu intercambio.".into();
            }
            AckOutcome::Wait
        }
    }

    fn receive_local_ok(session: &mut TradeSession) -> AckOutcome {
        session.phase = TradePhase::WaitingOk;
        if session.remote_ok {
            if session.slot.is_some() && session.remote.is_some() {
                AckOutcome::Complete
            } else {
                AckOutcome::Incomplete
            }
        } else {
            AckOutcome::Wait
        }
    }

    fn offered_session() -> TradeSession {
        let mut session = TradeSession::new(7, "Rival".into());
        session.slot = Some(2);
        session.local = Some(offer("local"));
        session.remote = Some(offer("remote"));
        session.phase = TradePhase::Confirm;
        session
    }

    #[test]
    fn remote_ok_before_local_offer_or_confirm_is_remembered() {
        // A remote OK can arrive while this side is still picking or checking
        // the offer. It must survive the later local offer/confirm so the local
        // OK completes the trade instead of waiting forever for a second OK.
        let mut session = TradeSession::new(7, "Rival".into());
        assert_eq!(receive_remote_ok(&mut session), AckOutcome::Wait);
        assert!(session.remote_ok);

        session.remote = Some(offer("remote"));
        session.local = Some(offer("local"));
        session.slot = Some(2);
        session.phase = TradePhase::Confirm;
        assert!(session.remote_ok);
        assert_eq!(receive_local_ok(&mut session), AckOutcome::Complete);
    }

    #[test]
    fn crossed_ok_completes_when_local_ok_arrives_last() {
        let mut session = offered_session();
        assert_eq!(receive_remote_ok(&mut session), AckOutcome::Wait);
        assert_eq!(
            session.message,
            "El rival acepto; confirma tu intercambio."
        );
        assert_eq!(receive_local_ok(&mut session), AckOutcome::Complete);
    }

    #[test]
    fn crossed_ok_completes_when_remote_ok_arrives_last() {
        let mut session = offered_session();
        assert_eq!(receive_local_ok(&mut session), AckOutcome::Wait);
        assert_eq!(session.phase, TradePhase::WaitingOk);
        assert_eq!(receive_remote_ok(&mut session), AckOutcome::Complete);
    }

    #[test]
    fn remote_ok_without_slot_or_offer_reports_an_incomplete_trade() {
        let mut session = TradeSession::new(7, "Rival".into());
        session.phase = TradePhase::WaitingOk;
        assert_eq!(receive_remote_ok(&mut session), AckOutcome::Incomplete);
    }

    #[test]
    fn commit_runs_apply_then_save_without_restore() {
        let mut state = Vec::new();
        let outcome = commit_transaction(
            &mut state,
            |state| {
                state.push("apply");
                Ok(())
            },
            |state| {
                state.push("save");
                Ok(())
            },
            |state| state.push("restore"),
        );
        assert_eq!(outcome, TradeCommit::Committed);
        assert_eq!(state, ["apply", "save"]);
    }

    #[test]
    fn apply_failure_skips_save_and_restores() {
        let mut state = Vec::new();
        let outcome = commit_transaction(
            &mut state,
            |state| {
                state.push("apply");
                Err("E_APPLY".into())
            },
            |state| {
                state.push("save");
                Ok(())
            },
            |state| state.push("restore"),
        );
        assert_eq!(outcome, TradeCommit::ApplyFailed("E_APPLY".into()));
        assert_eq!(state, ["apply", "restore"]);
    }

    #[test]
    fn save_failure_restores_after_the_attempt() {
        let mut state = Vec::new();
        let outcome = commit_transaction(
            &mut state,
            |state| {
                state.push("apply");
                Ok(())
            },
            |state| {
                state.push("save");
                Err("GuardarPartida".into())
            },
            |state| state.push("restore"),
        );
        assert_eq!(outcome, TradeCommit::SaveFailed("GuardarPartida".into()));
        assert_eq!(state, ["apply", "save", "restore"]);
    }

    #[test]
    fn failed_commit_restores_state_and_attempts_save_once() {
        #[derive(Default)]
        struct State {
            party: Vec<u8>,
            writes: usize,
            restores: usize,
        }

        let mut state = State {
            party: b"before".to_vec(),
            writes: 0,
            restores: 0,
        };
        let outcome = commit_transaction(
            &mut state,
            |state| {
                state.party = b"after".to_vec();
                state.writes += 1;
                Ok(())
            },
            |state| {
                state.writes += 1;
                Err("disk full".into())
            },
            |state| {
                state.party = b"before".to_vec();
                state.restores += 1;
            },
        );
        assert_eq!(outcome, TradeCommit::SaveFailed("disk full".into()));
        assert_eq!(state.party, b"before");
        assert_eq!(state.writes, 2);
        assert_eq!(state.restores, 1);
    }

    #[test]
    fn apply_failure_does_not_touch_state_without_restore_logic() {
        #[derive(Default)]
        struct State {
            party: Vec<u8>,
            writes: usize,
            restores: usize,
        }

        let mut state = State {
            party: b"before".to_vec(),
            writes: 0,
            restores: 0,
        };
        let outcome = commit_transaction(
            &mut state,
            |_| Err("rejected".into()),
            |state| {
                state.writes += 1;
                Ok(())
            },
            |state| state.restores += 1,
        );
        assert_eq!(outcome, TradeCommit::ApplyFailed("rejected".into()));
        assert_eq!(state.party, b"before");
        assert_eq!(state.writes, 0);
        assert_eq!(state.restores, 1);
    }
}
