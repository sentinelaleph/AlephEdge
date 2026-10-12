//! `DeskLink` unit tests.

use super::*;
use aleph_link::{ack_code, SkipNote};

#[derive(Default)]
struct FakeDesk {
    stopped: Vec<BotSelector>,
    closed: Vec<BotSelector>,
    started: Vec<BotSelector>,
    error: Option<AckReply>,
}

impl DeskControl for FakeDesk {
    async fn stop_opening(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.stopped.push(bot);
        Ok(AckReply::stopped(8))
    }
    async fn close_all(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.closed.push(bot);
        Ok(AckReply::closed(6, 0))
    }
    async fn start(&mut self, bot: BotSelector) -> Result<AckReply, AckReply> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.started.push(bot);
        Ok(AckReply::started(&[bot]))
    }
    fn status(&self) -> DeskStatus {
        DeskStatus {
            bots_running: vec!["futures".into()],
            open_positions: 8,
            today_net_pct: -1.2,
            skips_today: vec![SkipNote {
                reason: "comboFiltered".into(),
                count: 3,
            }],
            exposure_pct: 14.0,
            effective_bets: 2.0,
            kill_switch_tripped: false,
        }
    }
}

fn key() -> PairingKey {
    PairingKey::from_bytes([3u8; 32])
}
const NOW: u64 = 1_786_700_000_000;
/// The phone's session stamp.
const PHONE_EPOCH: u64 = 1_786_699_000_000;
/// The desk's session stamp.
const DESK_EPOCH: u64 = 1_786_699_500_000;

fn phone_envelope(seq: u64, cmd: &Command) -> String {
    phone_envelope_at_epoch(PHONE_EPOCH, seq, cmd)
}

fn phone_envelope_at_epoch(epoch: u64, seq: u64, cmd: &Command) -> String {
    serde_json::to_string(&seal(&key(), "desk-1", epoch, seq, NOW, cmd).unwrap()).unwrap()
}

fn desk(control: FakeDesk) -> DeskLink<FakeDesk> {
    DeskLink::new("desk-1", key(), control, DESK_EPOCH)
}

fn decode(env: &Envelope) -> DeskMessage {
    serde_json::from_str(&env.payload).unwrap()
}

fn handle(link: &mut DeskLink<FakeDesk>, raw: &str) -> Result<Vec<Envelope>, LinkError> {
    block_on(link.handle_raw(NOW, raw))
}

#[test]
fn every_command_produces_an_ack() {
    let mut link = desk(FakeDesk::default());
    let out = handle(
        &mut link,
        &phone_envelope(
            1,
            &Command::StopOpening {
                bot: BotSelector::All,
            },
        ),
    )
    .unwrap();
    match decode(&out[0]) {
        DeskMessage::Ack {
            for_seq,
            applied,
            detail,
            reply,
            ..
        } => {
            assert_eq!(for_seq, 1);
            assert!(applied);
            // Stable code + number; English fallback text for older phones.
            assert_eq!(reply, Some(AckReply::stopped(8)));
            assert_eq!(detail, AckReply::stopped(8).english());
        }
        other => panic!("expected an ack: {other:?}"),
    }
}

/// A command that could not be applied still returns an acknowledgement; it
/// does not stay SILENT. The phone must tell "tried and failed" from "no
/// answer at all".
#[test]
fn unapplied_command_still_returns_an_ack() {
    let fake_desk = FakeDesk {
        error: Some(AckReply::new(ack_code::START_FAILED)),
        ..Default::default()
    };
    let mut link = desk(fake_desk);
    let out = handle(
        &mut link,
        &phone_envelope(
            1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        ),
    )
    .unwrap();
    match decode(&out[0]) {
        DeskMessage::Ack {
            applied, reply, ..
        } => {
            assert!(!applied, "a failed command returned applied=true");
            assert_eq!(reply.map(|r| r.code).as_deref(), Some(ack_code::START_FAILED));
        }
        other => panic!("expected an ack: {other:?}"),
    }
}

/// Ambiguity is dangerous in an emergency control: "stop" does NOT TOUCH
/// open positions.
#[test]
fn stop_does_not_touch_open_positions() {
    let mut link = desk(FakeDesk::default());
    handle(
        &mut link,
        &phone_envelope(
            1,
            &Command::StopOpening {
                bot: BotSelector::Futures,
            },
        ),
    )
    .unwrap();
    assert_eq!(link.control().stopped, vec![BotSelector::Futures]);
    assert!(
        link.control().closed.is_empty(),
        "the stop command closed positions"
    );
}

#[test]
fn close_all_really_closes() {
    let mut link = desk(FakeDesk::default());
    handle(
        &mut link,
        &phone_envelope(
            1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        ),
    )
    .unwrap();
    assert_eq!(link.control().closed, vec![BotSelector::All]);
}

/// An envelope that fails verification produces neither an action nor a
/// REPLY (see `DeskLink::handle_raw`).
#[test]
fn envelope_with_bad_signature_neither_acts_nor_replies() {
    let mut link = desk(FakeDesk::default());
    let other_key = PairingKey::from_bytes([9u8; 32]);
    let env = seal(
        &other_key,
        "desk-1",
        PHONE_EPOCH,
        1,
        NOW,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    )
    .unwrap();
    let raw = serde_json::to_string(&env).unwrap();
    assert_eq!(handle(&mut link, &raw).unwrap_err(), LinkError::BadSignature);
    assert!(
        link.control().closed.is_empty(),
        "an unsigned command was APPLIED"
    );
}

#[test]
fn replayed_command_is_not_applied_twice() {
    let mut link = desk(FakeDesk::default());
    let raw = phone_envelope(
        4,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    );
    handle(&mut link, &raw).unwrap();
    assert!(matches!(
        handle(&mut link, &raw).unwrap_err(),
        LinkError::Replay { .. }
    ));
    assert_eq!(
        link.control().closed.len(),
        1,
        "the same command was applied twice"
    );
}

/// A restarted PHONE starts its counter from zero. Without the epoch the desk
/// would drop every command as a replay, "close all" included: the kill switch
/// would fail exactly when it is needed.
#[test]
fn restarted_phone_command_is_not_dropped() {
    let mut link = desk(FakeDesk::default());

    // First session: two commands.
    handle(&mut link, &phone_envelope(1, &Command::Status)).unwrap();
    handle(&mut link, &phone_envelope(2, &Command::Status)).unwrap();

    // The phone restarted: new epoch, counter back at 1.
    let restarted = phone_envelope_at_epoch(
        PHONE_EPOCH + 1,
        1,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    );
    handle(&mut link, &restarted).expect("the restarted phone's command was dropped");
    assert_eq!(link.control().closed, vec![BotSelector::All]);

    // A real replay is still rejected: the same envelope is not applied twice.
    assert!(matches!(
        handle(&mut link, &restarted).unwrap_err(),
        LinkError::Replay { .. }
    ));
    assert_eq!(
        link.control().closed.len(),
        1,
        "the same command was applied twice"
    );

    // And an envelope recorded in the OLD session does not pass.
    let old = phone_envelope_at_epoch(
        PHONE_EPOCH,
        99,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    );
    assert!(matches!(
        handle(&mut link, &old).unwrap_err(),
        LinkError::Replay { .. }
    ));
    assert_eq!(link.control().closed.len(), 1);
}

#[test]
fn status_query_returns_a_status_message() {
    let mut link = desk(FakeDesk::default());
    let out = handle(&mut link, &phone_envelope(1, &Command::Status)).unwrap();
    match decode(&out[0]) {
        DeskMessage::Status(st) => {
            assert_eq!(st.open_positions, 8);
            assert_eq!(st.effective_bets, 2.0);
        }
        other => panic!("expected a status: {other:?}"),
    }
}

#[test]
fn heartbeat_is_not_sent_early_but_on_time() {
    let mut link = desk(FakeDesk::default());
    assert!(link.heartbeat_due(NOW).is_some(), "the first beat must be sent");
    assert!(
        link.heartbeat_due(NOW + 1_000).is_none(),
        "a beat was sent again after 1 s"
    );
    assert!(link.heartbeat_due(NOW + HEARTBEAT_EVERY_MS).is_some());
}

/// The outgoing counter must advance with every message, or the PHONE's
/// replay protection would drop messages.
#[test]
fn outgoing_counter_advances_on_every_message() {
    let mut link = desk(FakeDesk::default());
    let a = link.heartbeat_due(NOW).unwrap();
    let b = handle(&mut link, &phone_envelope(1, &Command::Status))
        .unwrap()
        .remove(0);
    let c = link.heartbeat_due(NOW + HEARTBEAT_EVERY_MS).unwrap();
    assert!(a.seq < b.seq && b.seq < c.seq);
}

#[test]
fn desk_reply_verifies_on_the_phone_side() {
    let mut link = desk(FakeDesk::default());
    let out = handle(&mut link, &phone_envelope(1, &Command::Status)).unwrap();
    let mut phone_guard = ReplayGuard::new();
    let msg: DeskMessage = open(&key(), &mut phone_guard, NOW, &out[0]).unwrap();
    assert!(matches!(msg, DeskMessage::Status(_)));
}

/// Starting a bot must produce an acknowledgement too.
#[test]
fn start_produces_an_ack() {
    let mut link = desk(FakeDesk::default());
    let out = handle(
        &mut link,
        &phone_envelope(
            1,
            &Command::Start {
                bot: BotSelector::Spot,
            },
        ),
    )
    .unwrap();
    assert!(matches!(
        decode(&out[0]),
        DeskMessage::Ack { applied: true, .. }
    ));
    assert_eq!(link.control().started, vec![BotSelector::Spot]);
}

/// The relay can reflect the desk's own signed heartbeat back to it (both
/// ends use the same key). That must not make the phone's next "close all"
/// drop as a replay (see `aleph_link::open`).
#[test]
fn reflected_heartbeat_cannot_silence_the_kill_switch() {
    let mut link = desk(FakeDesk::default());
    let beat = link.heartbeat_due(NOW).expect("first beat");
    let reflection = serde_json::to_string(&beat).unwrap();
    assert!(handle(&mut link, &reflection).is_err(), "the reflection was accepted");

    let out = handle(
        &mut link,
        &phone_envelope(
            1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        ),
    )
    .expect("the phone's command was rejected");
    assert_eq!(out.len(), 1);
    assert_eq!(link.control().closed, vec![BotSelector::All]);
}

/// An envelope addressed to another desk (even with the same key on both
/// desks) is NOT applied here and does not advance the counter.
#[test]
fn other_desks_envelope_is_rejected() {
    let mut link = desk(FakeDesk::default());
    let env = seal(
        &key(),
        "desk-2",
        PHONE_EPOCH,
        1,
        NOW,
        &Command::CloseAll {
            bot: BotSelector::All,
        },
    )
    .unwrap();
    let raw = serde_json::to_string(&env).unwrap();
    assert_eq!(handle(&mut link, &raw).unwrap_err(), LinkError::WrongDesk);
    assert!(link.control().closed.is_empty(), "another desk's command was applied");

    // The counter did not advance: the same seq addressed to the right desk passes.
    handle(
        &mut link,
        &phone_envelope(
            1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        ),
    )
    .expect("the right desk's command was rejected");
    assert_eq!(link.control().closed, vec![BotSelector::All]);
}

/// A QR key that expires unused DIES: once the QR has left the screen,
/// someone who photographed it must not be able to run the desk with it.
#[test]
fn key_unused_by_its_deadline_dies() {
    let deadline = NOW + 120_000;
    let mut link = desk(FakeDesk::default()).with_claim_deadline(deadline);
    assert!(link.key_live(deadline));

    let command = |seq| {
        let env = seal(
            &key(),
            "desk-1",
            PHONE_EPOCH,
            seq,
            deadline + 1,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        )
        .unwrap();
        serde_json::to_string(&env).unwrap()
    };
    assert_eq!(
        block_on(link.handle_raw(deadline + 1, &command(1))).unwrap_err(),
        LinkError::KeyRevoked
    );
    assert!(link.control().closed.is_empty(), "a command was applied with a dead key");
    // A dead key sends no heartbeat either.
    assert!(link.heartbeat_due(deadline + 1).is_none());
    assert!(!link.key_live(deadline + 1));
}

/// A key used in time lives on after the QR dies: it is the paired phone's
/// lasting line.
#[test]
fn key_used_in_time_stays_alive() {
    let deadline = NOW + 120_000;
    let mut link = desk(FakeDesk::default()).with_claim_deadline(deadline);
    handle(&mut link, &phone_envelope(1, &Command::Status)).unwrap();

    let later = deadline + 3_600_000;
    let env = seal(&key(), "desk-1", PHONE_EPOCH, 2, later, &Command::Status).unwrap();
    let out = block_on(link.handle_raw(later, &serde_json::to_string(&env).unwrap()))
        .expect("the paired phone's command was rejected");
    assert_eq!(out.len(), 1);
    assert!(link.heartbeat_due(later).is_some());
}

/// Cancelling kills the key at once, even after a phone has paired.
#[test]
fn revoked_key_accepts_no_envelope() {
    let mut link = desk(FakeDesk::default());
    link.revoke();
    assert_eq!(
        handle(&mut link, &phone_envelope(1, &Command::Status)).unwrap_err(),
        LinkError::KeyRevoked
    );
    assert!(link.heartbeat_due(NOW).is_none());
}
