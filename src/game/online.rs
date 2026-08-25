// outer crate imports
use crate::render::util::RenderId;

/// online-match session state; `None` when not in an online game. The AI's
/// player-number constant is always 2 -- online play has no such fixed side,
/// since either peer may create the room (and so play as 1) or join it (as 2)
pub static mut ONLINE_SESSION: Option<OnlineSession> = None;

#[derive(Debug)]
pub struct OnlineSession {
    pub local_player_num: u32,
    pub connected: bool,
    /// clicks made so far during the local player's current turn, to be sent as
    /// one `action` message once the turn completes. RenderId::Confirm is never
    /// buffered (it's a pure local commit) and RenderId::Cancel discards
    /// whatever was buffered (nothing was actually committed) -- see the design
    /// spec's "Relaying moves" section for why.
    outgoing_buffer: Vec<RenderId>,
}

impl OnlineSession {
    pub fn new(local_player_num: u32) -> Self {
        OnlineSession {
            local_player_num,
            connected: false,
            outgoing_buffer: Vec::new(),
        }
    }

    pub fn record_click(&mut self, id: RenderId) {
        match id {
            RenderId::Confirm => {}
            RenderId::Cancel => self.outgoing_buffer.clear(),
            _ => self.outgoing_buffer.push(id),
        }
    }

    /// returns the buffered clicks and empties the buffer
    pub fn take_outgoing(&mut self) -> Vec<RenderId> {
        std::mem::take(&mut self.outgoing_buffer)
    }
}
