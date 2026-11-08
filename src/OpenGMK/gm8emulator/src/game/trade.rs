//! Trade session state machine and F3R adapter facade.
//!
//! Wire codec and validation policy live in `trade_codec` (clean-room, no game types).
//! F3R field/global mapping lives in `trade_adapter` (adapter boundary).
//!
//! The stock wire is kept as-is: a 49-field `OFFER` and empty `OK`/`NO` payloads.
//! Robustness against duplicate or crossed confirmations is session-local, so legacy
//! peers keep working without new tokens or bind messages.

use std::time::{Duration, Instant};

#[allow(unused_imports)]
pub use super::trade_adapter::{
    apply, capture, clear_slot, decode, encode, encode_message, first_empty_slot, is_empty_slot,
    is_valid_slot, occupied_slots, revalidate_source, slot_matches, validate_offer, TradeOffer,
    TradeValue, FIELD_COUNT, VALUE_COUNT,
};

/// Maximum inactive time in a trade before the session is cancelled. The timer is
/// refreshed by every accepted offer/confirmation, so a peer that answers keeps the
/// session alive while a silent one cannot leave the panel stuck.
pub const TRADE_TIMEOUT: Duration = Duration::from_secs(60);

/// Outcome of a session transition. `Duplicate` is intentionally a no-op: repeating an
/// already accepted `OK` or `NO` must not complete or cancel twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeAck {
    /// Message accepted; the session keeps waiting.
    Wait,
    /// Both confirmations are present and the caller may commit exactly once.
    Complete,
    /// A confirmation arrived without the selected slot, local offer or remote offer
    /// needed to apply it.
    Incomplete,
    /// Repeated or late confirmation for a session that already handled it.
    Duplicate,
    /// A first `NO` was accepted and the session is now closed.
    Accepted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradePhase {
    Pick,
    WaitingOffer,
    Confirm,
    WaitingOk,
}

/// Direction of a session. A gift has one offer only: the giver sends it and the
/// receiver confirms it without offering anything back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeKind {
    Trade,
    GiftGiver,
    GiftReceiver,
}

impl TradeKind {
    pub fn is_gift(self) -> bool {
        self != Self::Trade
    }

    pub fn is_giver(self) -> bool {
        self == Self::GiftGiver
    }

    pub fn is_receiver(self) -> bool {
        self == Self::GiftReceiver
    }
}

#[derive(Clone, Debug)]
pub struct TradeSession {
    pub peer: u64,
    pub name: String,
    pub kind: TradeKind,
    pub phase: TradePhase,
    pub slot: Option<u8>,
    pub local: Option<TradeOffer>,
    pub remote: Option<TradeOffer>,
    pub remote_ok: bool,
    pub local_ok: bool,
    pub selection: usize,
    pub message: String,
    /// Terminal after the peer's `NO`; late `OK`/`NO` messages are ignored.
    pub closed: bool,
    /// Set before the single apply/save attempt. A second commit is refused.
    pub committed: bool,
    /// Inactivity budget for this session.
    pub timeout: Duration,
    /// Last accepted local or remote transition.
    pub last_activity: Instant,
}

impl TradeSession {
    pub fn new(peer: u64, name: String) -> Self {
        Self::new_at(peer, name, Instant::now())
    }

    pub fn new_at(peer: u64, name: String, now: Instant) -> Self {
        Self::new_kind_at(peer, name, TradeKind::Trade, now)
    }

    pub fn new_gift_giver(peer: u64, name: String) -> Self {
        Self::new_gift_giver_at(peer, name, Instant::now())
    }

    pub fn new_gift_giver_at(peer: u64, name: String, now: Instant) -> Self {
        Self::new_kind_at(peer, name, TradeKind::GiftGiver, now)
    }

    pub fn new_gift_receiver(peer: u64, name: String) -> Self {
        Self::new_gift_receiver_at(peer, name, Instant::now())
    }

    pub fn new_gift_receiver_at(peer: u64, name: String, now: Instant) -> Self {
        let mut session = Self::new_kind_at(peer, name, TradeKind::GiftReceiver, now);
        // The receiver never picks a Pokemon; it waits for the giver's sole offer.
        session.phase = TradePhase::WaitingOffer;
        session.message = format!("Esperando el regalo de {}...", session.name);
        session
    }

    fn new_kind_at(peer: u64, name: String, kind: TradeKind, now: Instant) -> Self {
        Self {
            peer,
            name,
            kind,
            phase: TradePhase::Pick,
            slot: None,
            local: None,
            remote: None,
            remote_ok: false,
            local_ok: false,
            selection: 0,
            message: String::new(),
            closed: false,
            committed: false,
            timeout: TRADE_TIMEOUT,
            last_activity: now,
        }
    }

    pub fn is_gift(&self) -> bool {
        self.kind.is_gift()
    }

    #[allow(dead_code)]
    pub fn is_gift_giver(&self) -> bool {
        self.kind.is_giver()
    }

    pub fn is_gift_receiver(&self) -> bool {
        self.kind.is_receiver()
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Restart the inactivity timer after an accepted transition.
    pub fn touch(&mut self, now: Instant) {
        self.last_activity = now;
    }

    /// True when the session has been inactive for its whole timeout budget.
    /// Closed or already committed sessions do not time out.
    pub fn is_timed_out(&self, now: Instant) -> bool {
        if self.closed || self.committed {
            return false
        }
        now.saturating_duration_since(self.last_activity) >= self.timeout
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.timeout
            .saturating_sub(now.saturating_duration_since(self.last_activity))
    }

    fn completion(&self) -> TradeAck {
        match self.kind {
            TradeKind::Trade => {
                if self.slot.is_none() || self.local.is_none() {
                    return TradeAck::Incomplete
                }
                if self.remote.is_none() {
                    // Both confirmations may arrive before the peer's offer. Keep waiting
                    // instead of cancelling a session that can still complete.
                    return TradeAck::Wait
                }
            },
            TradeKind::GiftGiver => {
                // The giver never receives an offer; its own offer is also its
                // confirmation. Only the receiver's `OK` is outstanding.
                if self.slot.is_none() || self.local.is_none() {
                    return TradeAck::Incomplete
                }
            },
            TradeKind::GiftReceiver => {
                // The receiver never sends an offer. The giver's offer carries the
                // giver's confirmation, so only the local confirmation is missing.
                if self.remote.is_none() {
                    return TradeAck::Incomplete
                }
            },
        }
        if self.local_ok && self.remote_ok {
            TradeAck::Complete
        } else {
            TradeAck::Wait
        }
    }

    /// Records the local immutable offer once per session.
    pub fn bind_local_offer(&mut self, slot: u8, offer: TradeOffer) -> TradeAck {
        self.bind_local_offer_at(slot, offer, Instant::now())
    }

    pub fn bind_local_offer_at(
        &mut self,
        slot: u8,
        offer: TradeOffer,
        now: Instant,
    ) -> TradeAck {
        if self.closed
            || self.committed
            || self.kind.is_receiver()
            || self.local.is_some()
            || self.slot.is_some()
        {
            return TradeAck::Duplicate
        }
        self.slot = Some(slot);
        self.local = Some(offer);
        self.touch(now);
        if self.kind.is_giver() {
            // Sending the gift offer is also the giver's confirmation.
            self.local_ok = true;
            self.phase = TradePhase::WaitingOk;
            self.message = format!("Esperando la confirmacion de {}...", self.name);
            return self.completion()
        }
        if self.remote.is_some() {
            self.phase = TradePhase::Confirm;
            self.selection = 0;
            self.message = "Oferta recibida.".into();
        } else {
            self.phase = TradePhase::WaitingOffer;
            self.message = "Esperando la oferta del rival...".into();
        }
        TradeAck::Wait
    }

    /// Records the first remote offer. A repeated offer is ignored instead of
    /// replacing the one the user is already confirming.
    pub fn bind_remote_offer(&mut self, offer: TradeOffer) -> TradeAck {
        self.bind_remote_offer_at(offer, Instant::now())
    }

    pub fn bind_remote_offer_at(&mut self, offer: TradeOffer, now: Instant) -> TradeAck {
        if self.closed || self.committed || self.kind.is_giver() || self.remote.is_some() {
            return TradeAck::Duplicate
        }
        let label = offer.label.clone();
        self.remote = Some(offer);
        self.touch(now);
        if self.kind.is_receiver() {
            // The giver's sole offer is also the giver's confirmation.
            self.remote_ok = true;
            self.phase = TradePhase::Confirm;
            self.selection = 0;
            self.message = format!("{} te regala a {label}. Confirmas?", self.name);
            return self.completion()
        }
        match self.phase {
            TradePhase::WaitingOffer => {
                self.phase = TradePhase::Confirm;
                self.selection = 0;
                self.message = "Oferta recibida.".into();
                TradeAck::Wait
            },
            TradePhase::WaitingOk => {
                if self.remote_ok {
                    // A crossed early OK plus the now-arrived offer completes
                    // without waiting for a second OK that will never come.
                    let outcome = self.completion();
                    if outcome == TradeAck::Complete {
                        self.message = "Oferta recibida; intercambio listo.".into();
                    }
                    outcome
                } else {
                    self.message = "Oferta recibida; esperando la confirmacion del rival...".into();
                    TradeAck::Wait
                }
            },
            TradePhase::Pick => {
                self.message = "Oferta recibida; elige tu Pokemon.".into();
                TradeAck::Wait
            },
            TradePhase::Confirm => {
                self.message = "Oferta recibida.".into();
                TradeAck::Wait
            },
        }
    }

    pub fn note_remote_ok(&mut self) -> TradeAck {
        self.note_remote_ok_at(Instant::now())
    }

    pub fn note_remote_ok_at(&mut self, now: Instant) -> TradeAck {
        if self.closed || self.committed || self.remote_ok {
            return TradeAck::Duplicate
        }
        self.remote_ok = true;
        self.touch(now);
        match self.phase {
            TradePhase::WaitingOk => self.completion(),
            TradePhase::Confirm => {
                self.message = "El rival acepto; confirma tu intercambio.".into();
                TradeAck::Wait
            },
            TradePhase::Pick | TradePhase::WaitingOffer => TradeAck::Wait,
        }
    }

    pub fn note_remote_no(&mut self) -> TradeAck {
        self.note_remote_no_at(Instant::now())
    }

    pub fn note_remote_no_at(&mut self, now: Instant) -> TradeAck {
        if self.closed || self.committed {
            return TradeAck::Duplicate
        }
        self.closed = true;
        self.touch(now);
        TradeAck::Accepted
    }

    pub fn note_local_ok(&mut self) -> TradeAck {
        self.note_local_ok_at(Instant::now())
    }

    pub fn note_local_ok_at(&mut self, now: Instant) -> TradeAck {
        if self.closed || self.committed || self.local_ok {
            return TradeAck::Duplicate
        }
        self.local_ok = true;
        self.phase = TradePhase::WaitingOk;
        self.touch(now);
        if self.remote_ok {
            let outcome = self.completion();
            if outcome == TradeAck::Wait {
                self.message = "Esperando la oferta del rival...".into();
            }
            outcome
        } else {
            self.message = "Esperando la confirmacion del rival...".into();
            TradeAck::Wait
        }
    }

    /// Claims the one allowed commit attempt for this session.
    pub fn begin_commit(&mut self) -> bool {
        if self.closed || self.committed {
            return false
        }
        self.committed = true;
        true
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
/// after either failure, before the error is returned to the caller. A failed
/// restore is appended to the failure reason so callers can distinguish a clean
/// rollback from a partial one.
pub(crate) fn commit_transaction<S, A, B, R>(
    state: &mut S,
    apply: A,
    save: B,
    restore: R,
) -> TradeCommit
where
    A: FnOnce(&mut S) -> Result<(), String>,
    B: FnOnce(&mut S) -> Result<(), String>,
    R: FnOnce(&mut S) -> Result<(), String>,
{
    match apply(state) {
        Ok(()) => match save(state) {
            Ok(()) => TradeCommit::Committed,
            Err(error) => TradeCommit::SaveFailed(merge_rollback(error, restore(state))),
        },
        Err(error) => TradeCommit::ApplyFailed(merge_rollback(error, restore(state))),
    }
}

fn merge_rollback(error: String, rollback: Result<(), String>) -> String {
    match rollback {
        Ok(()) => error,
        Err(rollback) => format!("{error}; rollback failed: {rollback}"),
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

    fn at(millis: u64) -> Instant {
        Instant::now() + Duration::from_millis(millis)
    }

    fn offered_session(now: Instant) -> TradeSession {
        let mut session = TradeSession::new_at(7, "Rival".into(), now);
        assert_eq!(
            session.bind_local_offer_at(2, offer("local"), now),
            TradeAck::Wait
        );
        assert_eq!(
            session.bind_remote_offer_at(offer("remote"), now),
            TradeAck::Wait
        );
        session
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
        assert!(!session.local_ok);
        assert!(!session.closed);
        assert!(!session.committed);
        assert_eq!(session.selection, 0);
        assert!(session.message.is_empty());
        assert_eq!(session.timeout, TRADE_TIMEOUT);
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
        first.local_ok = true;

        assert_eq!(second.peer, 2);
        assert_eq!(second.phase, TradePhase::Pick);
        assert_eq!(second.slot, None);
        assert!(second.local.is_none());
        assert!(second.remote.is_none());
        assert!(!second.local_ok);
        assert_eq!(second.selection, 0);
    }

    #[test]
    fn remote_ok_before_local_offer_or_confirm_is_remembered() {
        // A remote OK can arrive while this side is still picking or checking
        // the offer. It must survive the later local offer/confirm so the local
        // OK completes the trade instead of waiting forever for a second OK.
        let now = at(0);
        let mut session = TradeSession::new_at(7, "Rival".into(), now);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Wait);
        assert!(session.remote_ok);

        assert_eq!(
            session.bind_remote_offer_at(offer("remote"), now),
            TradeAck::Wait
        );
        assert_eq!(
            session.bind_local_offer_at(2, offer("local"), now),
            TradeAck::Wait
        );
        assert!(session.remote_ok);
        assert_eq!(session.note_local_ok_at(now), TradeAck::Complete);
    }

    #[test]
    fn crossed_ok_completes_when_local_ok_arrives_last() {
        let now = at(0);
        let mut session = offered_session(now);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Wait);
        assert_eq!(
            session.message,
            "El rival acepto; confirma tu intercambio."
        );
        assert_eq!(session.note_local_ok_at(now), TradeAck::Complete);
    }

    #[test]
    fn crossed_ok_completes_when_remote_ok_arrives_last() {
        let now = at(0);
        let mut session = offered_session(now);
        assert_eq!(session.note_local_ok_at(now), TradeAck::Wait);
        assert_eq!(session.phase, TradePhase::WaitingOk);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Complete);
    }

    #[test]
    fn gift_giver_offer_is_also_its_confirmation() {
        let now = at(0);
        let mut session = TradeSession::new_gift_giver_at(7, "Rival".into(), now);
        assert!(session.is_gift());
        assert!(session.is_gift_giver());
        assert_eq!(
            session.bind_local_offer_at(2, offer("gift"), now),
            TradeAck::Wait
        );
        assert!(session.local_ok);
        assert_eq!(session.phase, TradePhase::WaitingOk);
        assert!(session.remote.is_none());
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Complete);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Duplicate);
    }

    #[test]
    fn gift_giver_does_not_accept_a_remote_offer() {
        let now = at(0);
        let mut session = TradeSession::new_gift_giver_at(7, "Rival".into(), now);
        assert_eq!(
            session.bind_remote_offer_at(offer("remote"), now),
            TradeAck::Duplicate
        );
        assert!(session.remote.is_none());
    }

    #[test]
    fn gift_receiver_waits_for_the_givers_offer_then_confirms() {
        let now = at(0);
        let mut session = TradeSession::new_gift_receiver_at(7, "Rival".into(), now);
        assert!(session.is_gift_receiver());
        assert_eq!(session.phase, TradePhase::WaitingOffer);
        assert_eq!(
            session.bind_remote_offer_at(offer("regalo"), now),
            TradeAck::Wait
        );
        assert!(session.remote_ok);
        assert_eq!(session.phase, TradePhase::Confirm);
        assert!(session.local.is_none());
        assert_eq!(session.note_local_ok_at(now), TradeAck::Complete);
    }

    #[test]
    fn gift_receiver_cannot_send_an_offer() {
        let now = at(0);
        let mut session = TradeSession::new_gift_receiver_at(7, "Rival".into(), now);
        assert_eq!(
            session.bind_local_offer_at(2, offer("local"), now),
            TradeAck::Duplicate
        );
        assert!(session.local.is_none());
        assert!(session.slot.is_none());
    }

    #[test]
    fn duplicate_gift_offer_keeps_the_first_one() {
        let now = at(0);
        let mut session = TradeSession::new_gift_receiver_at(7, "Rival".into(), now);
        assert_eq!(
            session.bind_remote_offer_at(offer("first"), now),
            TradeAck::Wait
        );
        assert_eq!(
            session.bind_remote_offer_at(offer("second"), now),
            TradeAck::Duplicate
        );
        assert_eq!(session.remote.as_ref().unwrap().label, "first");
    }

    #[test]
    fn gift_receiver_no_closes_without_a_local_offer() {
        let now = at(0);
        let mut session = TradeSession::new_gift_receiver_at(7, "Rival".into(), now);
        assert_eq!(
            session.bind_remote_offer_at(offer("regalo"), now),
            TradeAck::Wait
        );
        assert_eq!(session.note_remote_no_at(now), TradeAck::Accepted);
        assert!(session.closed);
        assert!(!session.begin_commit());
    }

    #[test]
    fn offer_arriving_after_both_oks_completes_waiting_ok() {
        let now = at(0);
        let mut session = TradeSession::new_at(7, "Rival".into(), now);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Wait);
        assert_eq!(
            session.bind_local_offer_at(2, offer("local"), now),
            TradeAck::Wait
        );
        assert_eq!(session.note_local_ok_at(now), TradeAck::Wait);
        assert_eq!(session.phase, TradePhase::WaitingOk);
        assert_eq!(
            session.bind_remote_offer_at(offer("remote"), now),
            TradeAck::Complete
        );
    }

    #[test]
    fn remote_ok_without_slot_or_offer_reports_an_incomplete_trade() {
        let now = at(0);
        let mut session = TradeSession::new_at(7, "Rival".into(), now);
        session.phase = TradePhase::WaitingOk;
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Incomplete);
    }

    #[test]
    fn duplicate_remote_ok_is_idempotent() {
        let now = at(0);
        let mut session = offered_session(now);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Wait);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Duplicate);
        assert_eq!(session.note_local_ok_at(now), TradeAck::Complete);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Duplicate);
    }

    #[test]
    fn duplicate_local_ok_is_idempotent() {
        let now = at(0);
        let mut session = offered_session(now);
        assert_eq!(session.note_local_ok_at(now), TradeAck::Wait);
        assert_eq!(session.note_local_ok_at(now), TradeAck::Duplicate);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Complete);
        assert_eq!(session.note_local_ok_at(now), TradeAck::Duplicate);
    }

    #[test]
    fn duplicate_no_is_idempotent() {
        let now = at(0);
        let mut session = offered_session(now);
        assert_eq!(session.note_remote_no_at(now), TradeAck::Accepted);
        assert!(session.closed);
        assert_eq!(session.note_remote_no_at(now), TradeAck::Duplicate);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Duplicate);
        assert_eq!(session.note_local_ok_at(now), TradeAck::Duplicate);
    }

    #[test]
    fn cancel_after_local_ok_prevents_completion() {
        let now = at(0);
        let mut session = offered_session(now);
        assert_eq!(session.note_local_ok_at(now), TradeAck::Wait);
        assert_eq!(session.phase, TradePhase::WaitingOk);
        assert_eq!(session.note_remote_no_at(now), TradeAck::Accepted);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Duplicate);
        assert_ne!(session.note_local_ok_at(now), TradeAck::Complete);
    }

    #[test]
    fn cancel_after_remote_ok_before_commit_is_still_accepted() {
        // trade_tick drains the whole peer batch before committing, so an OK
        // followed by a NO in that batch cancels instead of applying.
        let now = at(0);
        let mut session = offered_session(now);
        assert_eq!(session.note_local_ok_at(now), TradeAck::Wait);
        assert_eq!(session.note_remote_ok_at(now), TradeAck::Complete);
        assert_eq!(session.note_remote_no_at(now), TradeAck::Accepted);
        assert!(session.closed);
        assert!(!session.begin_commit());
    }

    #[test]
    fn duplicate_remote_offer_keeps_the_first_one() {
        let now = at(0);
        let mut session = TradeSession::new_at(7, "Rival".into(), now);
        assert_eq!(
            session.bind_remote_offer_at(offer("first"), now),
            TradeAck::Wait
        );
        assert_eq!(
            session.bind_remote_offer_at(offer("second"), now),
            TradeAck::Duplicate
        );
        assert_eq!(session.remote.as_ref().unwrap().label, "first");
    }

    #[test]
    fn duplicate_local_offer_is_refused() {
        let now = at(0);
        let mut session = TradeSession::new_at(7, "Rival".into(), now);
        assert_eq!(
            session.bind_local_offer_at(1, offer("first"), now),
            TradeAck::Wait
        );
        assert_eq!(
            session.bind_local_offer_at(2, offer("second"), now),
            TradeAck::Duplicate
        );
        assert_eq!(session.slot, Some(1));
        assert_eq!(session.local.as_ref().unwrap().label, "first");
    }

    #[test]
    fn timeout_is_explicit_and_refreshed_by_activity() {
        let start = at(0);
        let mut session = TradeSession::new_at(7, "Rival".into(), start);
        session.set_timeout(Duration::from_secs(5));

        assert!(!session.is_timed_out(start + Duration::from_millis(4_999)));
        assert!(session.is_timed_out(start + Duration::from_secs(5)));

        session.touch(start + Duration::from_secs(4));
        assert!(!session.is_timed_out(start + Duration::from_secs(8)));
        assert!(session.is_timed_out(start + Duration::from_secs(9)));
    }

    #[test]
    fn waiting_ok_times_out_instead_of_waiting_forever() {
        let start = at(0);
        let mut session = offered_session(start);
        assert_eq!(session.note_local_ok_at(start), TradeAck::Wait);
        assert_eq!(session.phase, TradePhase::WaitingOk);
        assert!(!session.is_timed_out(start));
        assert!(session.is_timed_out(start + TRADE_TIMEOUT));
        assert!(!session.closed);
    }

    #[test]
    fn closed_or_committed_sessions_do_not_time_out() {
        let start = at(0);
        let mut closed = TradeSession::new_at(7, "Rival".into(), start);
        assert_eq!(closed.note_remote_no_at(start), TradeAck::Accepted);
        assert!(!closed.is_timed_out(start + TRADE_TIMEOUT));

        let mut committed = offered_session(start);
        assert!(committed.begin_commit());
        assert!(!committed.begin_commit());
        assert!(!committed.is_timed_out(start + TRADE_TIMEOUT));
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
            |state| {
                state.push("restore");
                Ok(())
            },
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
            |state| {
                state.push("restore");
                Ok(())
            },
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
            |state| {
                state.push("restore");
                Ok(())
            },
        );
        assert_eq!(outcome, TradeCommit::SaveFailed("GuardarPartida".into()));
        assert_eq!(state, ["apply", "save", "restore"]);
    }

    #[test]
    fn save_failure_reports_a_failed_restore() {
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
            |state| {
                state.push("restore");
                Err("estado inconsistente".into())
            },
        );
        assert_eq!(
            outcome,
            TradeCommit::SaveFailed(
                "GuardarPartida; rollback failed: estado inconsistente".into()
            )
        );
        assert_eq!(state, ["apply", "save", "restore"]);
    }

    #[test]
    fn apply_failure_reports_a_failed_restore_without_saving() {
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
            |state| {
                state.push("restore");
                Err("estado inconsistente".into())
            },
        );
        assert_eq!(
            outcome,
            TradeCommit::ApplyFailed("E_APPLY; rollback failed: estado inconsistente".into())
        );
        assert_eq!(state, ["apply", "restore"]);
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
                Ok(())
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
            |state| {
                state.restores += 1;
                Ok(())
            },
        );
        assert_eq!(outcome, TradeCommit::ApplyFailed("rejected".into()));
        assert_eq!(state.party, b"before");
        assert_eq!(state.writes, 0);
        assert_eq!(state.restores, 1);
    }
}
