//! The desktop side of the link layer: verifies an incoming command, applies
//! it, and **always** returns an acknowledgement.
//!
//! # Why a separate crate
//!
//! It is tested in full through the `DeskControl` trait, without a Tauri app
//! or the real bot desk. The app only wires `DeskControl` to its desk
//! (`src-tauri/src/link`).
//!
//! # Invariants
//!
//! 1. **Every command produces an acknowledgement.** If it was not applied,
//!    the acknowledgement says `applied: false` and why. Without one the phone
//!    cannot show a green tick; a user who pressed and believes the desk
//!    stopped when it did not is worse off than one with no kill switch.
//! 2. **Stop and close-all are never merged.** Ambiguity is costly in an
//!    emergency control (see `aleph_link::Command::StopOpening`).
//! 3. **An envelope that fails verification does nothing** and does not
//!    advance the counter.

pub mod pairing;
pub mod session;

use aleph_link::{
    open, seal, AckReply, BotSelector, Command, DeskMessage, DeskStatus, Envelope, LinkError,
    PairingKey, ReplayGuard,
};

/// The desk's real controls.
///
/// The methods are **async** because closing open positions goes to the
/// exchange and takes time. Were they synchronous, the app would have to push
/// the work to the background and return at once, so the acknowledgement
/// would go out BEFORE the closing FINISHED. An early acknowledgement means
/// the phone reads "6 positions closed" while the positions are still open:
/// exactly the failure this layer exists to prevent.
///
/// Results are `AckReply` codes, not sentences (see `aleph_link::AckReply`).
pub trait DeskControl {
    /// Stops opening new positions. Does NOT TOUCH open positions.
    fn stop_opening(
        &mut self,
        bot: BotSelector,
    ) -> impl std::future::Future<Output = Result<AckReply, AckReply>>;
    /// Closes the open positions as well. Returns once the work is DONE.
    fn close_all(
        &mut self,
        bot: BotSelector,
    ) -> impl std::future::Future<Output = Result<AckReply, AckReply>>;
    /// Starts the bot.
    fn start(
        &mut self,
        bot: BotSelector,
    ) -> impl std::future::Future<Output = Result<AckReply, AckReply>>;
    /// Current state.
    fn status(&self) -> DeskStatus;
}

/// Heartbeat interval (ms). The relay counts the desk as silent after three
/// missed beats.
pub const HEARTBEAT_EVERY_MS: u64 = 30_000;

pub struct DeskLink<C: DeskControl> {
    desk_id: String,
    key: PairingKey,
    guard: ReplayGuard,
    /// This session's stamp. `out_seq` restarts from zero with every session,
    /// so the epoch is what tells the phone a restarted desk from a replay
    /// (see `aleph_link::ReplayGuard`).
    epoch: u64,
    /// Counter of outgoing messages. The phone uses it in its own replay
    /// protection.
    out_seq: u64,
    control: C,
    last_beat_ms: u64,
    /// By this moment (the QR's expiry) a phone must prove it HOLDS the key.
    /// If none does, the key dies: a desk that kept accepting commands signed
    /// with it after the QR left the screen would leave anyone who
    /// photographed the QR a door that never closes.
    claim_deadline_ms: Option<u64>,
    /// Whether a verified phone envelope has arrived.
    claimed: bool,
    /// The key is revoked (expired or cancelled). There is no way back.
    revoked: bool,
}

impl<C: DeskControl> DeskLink<C> {
    /// `session_epoch` is this session's stamp: in production, the wall-clock
    /// time (ms) at which the session started. It only has to move forward; an
    /// epoch that goes backwards makes the phone reject this desk as a replay.
    pub fn new(
        desk_id: impl Into<String>,
        key: PairingKey,
        control: C,
        session_epoch: u64,
    ) -> Self {
        Self {
            desk_id: desk_id.into(),
            key,
            guard: ReplayGuard::new(),
            epoch: session_epoch,
            out_seq: 0,
            control,
            last_beat_ms: 0,
            claim_deadline_ms: None,
            claimed: false,
            revoked: false,
        }
    }

    /// The moment (ms) at which the key dies if no phone has used it. Sessions
    /// set up by a pairing QR tie this to the QR's expiry.
    pub fn with_claim_deadline(mut self, deadline_ms: u64) -> Self {
        self.claim_deadline_ms = Some(deadline_ms);
        self
    }

    /// Revokes the key for good (the pairing was cancelled). From then on no
    /// envelope is accepted and no heartbeat is sent.
    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    /// Whether the key is still valid. If its deadline passed before any use,
    /// it dies here for good; a later envelope cannot revive it.
    pub fn key_live(&mut self, now_ms: u64) -> bool {
        if !self.revoked && !self.claimed {
            if let Some(deadline) = self.claim_deadline_ms {
                if now_ms > deadline {
                    self.revoked = true;
                }
            }
        }
        !self.revoked
    }

    fn next_seq(&mut self) -> u64 {
        self.out_seq += 1;
        self.out_seq
    }

    fn send(&mut self, now_ms: u64, msg: &DeskMessage) -> Envelope {
        let seq = self.next_seq();
        seal(
            &self.key,
            &self.desk_id.clone(),
            self.epoch,
            seq,
            now_ms,
            msg,
        )
        .expect("DeskMessage always serializes")
    }

    /// Handles a raw envelope from the relay.
    ///
    /// Returns the envelope(s) to send to the phone. When verification fails
    /// it returns an error and **no** envelope: an envelope that fails
    /// verification produces neither an action nor a reply. A reply would let
    /// anyone sending unsigned envelopes probe for the desk and its state.
    pub async fn handle_raw(&mut self, now_ms: u64, raw: &str) -> Result<Vec<Envelope>, LinkError> {
        if !self.key_live(now_ms) {
            return Err(LinkError::KeyRevoked);
        }
        let env: Envelope =
            serde_json::from_str(raw).map_err(|e| LinkError::Malformed(e.to_string()))?;
        // An envelope addressed to another desk is not applied here, even if
        // its signature holds (the same key used on two desks). Checked BEFORE
        // the counter.
        if env.desk_id != self.desk_id {
            return Err(LinkError::WrongDesk);
        }
        let cmd: Command = open(&self.key, &mut self.guard, now_ms, &env)?;
        self.claimed = true;
        Ok(vec![self.apply(now_ms, env.seq, cmd).await])
    }

    /// Applies a verified command and produces the acknowledgement.
    async fn apply(&mut self, now_ms: u64, for_seq: u64, cmd: Command) -> Envelope {
        let (applied, reply) = match cmd {
            Command::Status => {
                let st = self.control.status();
                // A status query is answered with its own message type. It is
                // still never left unanswered: the phone must tell "I asked and
                // got nothing" from "I asked and the desk answered".
                return self.send(now_ms, &DeskMessage::Status(st));
            }
            // The `.await` is essential: the acknowledgement goes out once the
            // work is DONE (see `DeskControl`).
            Command::StopOpening { bot } => match self.control.stop_opening(bot).await {
                Ok(d) => (true, d),
                Err(e) => (false, e),
            },
            Command::CloseAll { bot } => match self.control.close_all(bot).await {
                Ok(d) => (true, d),
                Err(e) => (false, e),
            },
            Command::Start { bot } => match self.control.start(bot).await {
                Ok(d) => (true, d),
                Err(e) => (false, e),
            },
        };
        self.send(
            now_ms,
            &DeskMessage::Ack {
                for_seq,
                applied,
                at_ms: now_ms,
                // English fallback for older phones that do not know the code.
                detail: reply.english(),
                reply: Some(reply),
            },
        )
    }

    /// Produces a heartbeat envelope when one is due.
    ///
    /// Returns `None` until `HEARTBEAT_EVERY_MS` has passed since the last
    /// beat. Beats are not sent more often: the relay counts EVERY carried
    /// message as a sign of life.
    pub fn heartbeat_due(&mut self, now_ms: u64) -> Option<Envelope> {
        // A beat with a dead key would show the phone a line that no longer
        // exists as live.
        if !self.key_live(now_ms) {
            return None;
        }
        if now_ms.saturating_sub(self.last_beat_ms) < HEARTBEAT_EVERY_MS {
            return None;
        }
        self.last_beat_ms = now_ms;
        Some(self.send(now_ms, &DeskMessage::Heartbeat { at_ms: now_ms }))
    }

    pub fn control(&self) -> &C {
        &self.control
    }
}

/// A dependency-free `block_on`.
///
/// Drives `DeskControl` futures from tests and other synchronous callers. It
/// is enough for futures that do no real I/O; Aleph Edge uses its own runtime
/// (`tauri::async_runtime`).
pub fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::hint::spin_loop(),
        }
    }
}

#[cfg(test)]
mod tests;
