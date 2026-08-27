// wasm-bindgen imports
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
// outer crate imports
use crate::render::util::RenderId;
// root imports
use crate::{GAME, MENU};

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

#[wasm_bindgen(module = "/js/online.js")]
extern "C" {
    #[wasm_bindgen(js_name = js_create_room)]
    fn js_create_room() -> String;
    #[wasm_bindgen(js_name = js_join_room)]
    fn js_join_room(code: String);
    #[wasm_bindgen(js_name = js_leave_room)]
    fn js_leave_room();
    #[wasm_bindgen(js_name = js_send_init)]
    fn js_send_init(deck: Vec<u8>, hand1: Vec<u8>, hand2: Vec<u8>, settings: Vec<u8>);
    #[wasm_bindgen(js_name = js_send_action)]
    fn js_send_action(clicks: Vec<String>);
    #[wasm_bindgen(js_name = getRoomCodeFromUrl)]
    fn get_room_code_from_url() -> Option<String>;
    #[wasm_bindgen(js_name = js_copy_to_clipboard)]
    fn js_copy_to_clipboard(text: String);
}

/// starts hosting a room, returning the code to show the player. Leaves any
/// room already open first -- neither online-choice sub-panel is ever hidden
/// again once shown (see reset_online_panels), so a player can reach this
/// (eg. by opening "Join Game" after already creating one, or just clicking
/// "Create Game" twice) without having backed out through the menu first.
/// Without this, the previous room's Trystero connection would leak (never
/// left) and a peer connecting to it late could still call on_peer_connected
/// against whatever ONLINE_SESSION this call replaces it with
pub fn create_room() -> String {
    if unsafe { ONLINE_SESSION.is_some() } {
        js_leave_room();
    }
    let code = js_create_room();
    unsafe { ONLINE_SESSION = Some(OnlineSession::new(1)) };
    code
}

/// attempts to join an existing room by its code -- see create_room's doc on
/// why any already-open room is left first
pub fn join_room(code: String) {
    if unsafe { ONLINE_SESSION.is_some() } {
        js_leave_room();
    }
    js_join_room(code);
    unsafe { ONLINE_SESSION = Some(OnlineSession::new(2)) };
}

pub fn leave_room() {
    js_leave_room();
    unsafe { ONLINE_SESSION = None };
    reset_online_panels();
    if let Some(status) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("online-status"))
    {
        status.set_text_content(Some(""));
    }
}

/// hides the create/join sub-panels and clears their room-code/input text.
/// Menu::activate only ever toggles the top-level "Play Online" panel, never
/// these two -- left alone, whichever one a previous visit opened (showing a
/// now-defunct room code, or "Waiting for opponent...") would still be
/// showing the next time the player returns to "Play Online", as if that old
/// attempt were still live. Called whenever a room is actually left (see
/// leave_room, on_peer_disconnected) so the next visit starts clean
fn reset_online_panels() {
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(document) => document,
        None => return,
    };
    if let Some(el) = document.get_element_by_id("online-create-panel") {
        el.set_attribute("hidden", "true").ok();
    }
    if let Some(el) = document.get_element_by_id("online-join-panel") {
        el.set_attribute("hidden", "true").ok();
    }
    if let Some(el) = document.get_element_by_id("online-room-code") {
        el.set_text_content(Some(""));
    }
    if let Some(input) = document
        .get_element_by_id("online-join-code-input")
        .and_then(|el| el.dyn_into::<web_sys::HtmlInputElement>().ok())
    {
        input.set_value("");
    }
}

pub fn room_code_from_url() -> Option<String> {
    get_room_code_from_url()
}

/// copies `text` to the clipboard via a small JS wrapper -- web-sys's Clipboard
/// bindings are gated behind the `web_sys_unstable_apis` rustc cfg flag, which
/// would mean a project-wide build config change just for a copy button, so
/// this goes through js/online.js the same way Trystero itself does. Errors
/// (eg. permission denied) are ignored -- this is a "nice to have" convenience
/// button, not a critical action
pub fn copy_to_clipboard(text: String) {
    js_copy_to_clipboard(text);
}

/// called from JS when the peer connects. The host (player 1) generates the
/// match now and sends it whole; the joiner (player 2) just waits for `init`
#[wasm_bindgen]
pub fn on_peer_connected() {
    let session = unsafe { ONLINE_SESSION.as_mut() };
    let is_host = match session {
        Some(session) if !session.connected => {
            session.connected = true;
            session.local_player_num == 1
        }
        // already connected, or no session at all -- ignore. Trystero's
        // torrent strategy churns through many speculative RTCPeerConnections
        // per room, and a stray extra onPeerJoin (or a third peer finding the
        // same room code) must never regenerate and resend a brand new game
        // over one already in progress
        _ => return,
    };

    if is_host {
        let game = crate::game::structs::Game::new();
        let deck = crate::game::card_encoding::cards_to_bytes(&game.deck);
        let hand1 = crate::game::card_encoding::cards_to_bytes(&game.player_1);
        let hand2 = crate::game::card_encoding::cards_to_bytes(&game.player_2);
        
        // Serialize settings to bytes (mixed bool and u8 array)
        let settings_bytes = unsafe {
            vec![
                if crate::game::flags::ALLOW_LINEAR_DEPENDENCE { 1 } else { 0 },
                crate::game::flags::ALLOW_LIMITS_BEYOND_BOUNDS,
                crate::game::flags::INVERSE_TRIG_PRINCIPAL_VALUE,
                if crate::game::flags::FULL_COMPUTE { 1 } else { 0 },
                if crate::game::flags::DISPLAY_LN_FOR_LOG { 1 } else { 0 },
                if crate::game::flags::USE_FRACTIONAL_EXPONENTS { 1 } else { 0 },
                if crate::game::flags::LIMIT_FIELD_BASIS { 1 } else { 0 },
                if crate::game::flags::CONFIRM_BEFORE_PLAY { 1 } else { 0 },
            ]
        };
        
        js_send_init(deck, hand1, hand2, settings_bytes);
        start_online_game(game);
    }
}

/// Apply settings received from host (called from JS)
#[wasm_bindgen]
pub fn apply_settings_from_host(settings_bytes: Vec<u8>) {
    if settings_bytes.len() >= 8 {
        let settings = crate::game::flags::GameSettings {
            allow_linear_dependence: settings_bytes[0] == 1,
            allow_limits_beyond_bounds: settings_bytes[1],
            inverse_trig_principal_value: settings_bytes[2],
            full_compute: settings_bytes[3] == 1,
            display_ln_for_log: settings_bytes[4] == 1,
            use_fractional_exponents: settings_bytes[5] == 1,
            limit_field_basis: settings_bytes[6] == 1,
            confirm_before_play: settings_bytes[7] == 1,
        };
        crate::game::flags::apply_settings(&settings);
    }
}

/// called from JS when the peer disconnects, at any point -- v1 has no
/// reconnection, so this always ends the match
#[wasm_bindgen]
pub fn on_peer_disconnected() {
    let was_connected = unsafe { ONLINE_SESSION.as_ref() }
        .map(|s| s.connected)
        .unwrap_or(false);
    unsafe { ONLINE_SESSION = None };

    if was_connected {
        if let Some(menu) = unsafe { MENU.as_ref() } {
            // clears the now-defunct room code/create-join panels first, so
            // returning to "Play Online" shows the disconnect message against
            // the choice screen, not a stale room code implying the old room
            // is still live
            reset_online_panels();
            if let Some(status) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("online-status"))
            {
                status.set_text_content(Some(
                    "Your opponent disconnected. Returning to the menu.",
                ));
            }
            if let Some(game) = unsafe { GAME.as_mut() } {
                game.set_state(crate::game::structs::GameState::MENU);
            }
            menu.activate("PLAYONLINE".to_string());
        }
    }
}

/// called from JS when Trystero reports the room could not be joined at all,
/// or no peer connected within the timeout
#[wasm_bindgen]
pub fn on_connection_error() {
    unsafe { ONLINE_SESSION = None };
    if let Some(status) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("online-status"))
    {
        status.set_text_content(Some("Couldn't connect. Please try again."));
    }
}

/// called from JS when the `init` message arrives (joiner only)
#[wasm_bindgen]
pub fn on_init_received(deck: Vec<u8>, hand1: Vec<u8>, hand2: Vec<u8>) {
    let deck = match crate::game::card_encoding::bytes_to_cards(&deck) {
        Some(cards) => cards,
        None => return, // malformed message; ignore rather than panic
    };
    let player_1 = match crate::game::card_encoding::bytes_to_cards(&hand1) {
        Some(cards) => cards,
        None => return,
    };
    let player_2 = match crate::game::card_encoding::bytes_to_cards(&hand2) {
        Some(cards) => cards,
        None => return,
    };

    let game = crate::game::structs::Game::from_online_parts(deck, player_1, player_2);
    start_online_game(game);
}

/// installs the synced game and switches both the game state and rendering
/// over to it -- shared by both the host (after generating) and the joiner
/// (after receiving). This is the moment the match actually starts, so unlike
/// the earlier "Play Online" button click (which only opened the create/join
/// panel), this is the point that closes the menu and draws the board.
/// `game.state` is set directly rather than via `Game::set_state`, since
/// `set_state` intentionally does *not* treat `PLAYONLINE` as a
/// close-and-draw trigger (that would fire prematurely on the button click,
/// before any connection exists)
fn start_online_game(mut game: crate::game::structs::Game) {
    use crate::game::structs::GameState;
    game.state = GameState::PLAYONLINE;
    unsafe { GAME = Some(game) };
    if let Some(menu) = unsafe { MENU.as_ref() } {
        menu.close();
    }
    // card positions (which side each hand renders on) are normally only
    // recalculated on window resize -- but which side is "local" now depends on
    // ONLINE_SESSION.local_player_num, which didn't exist yet the last time that ran
    // (page load, still GameState::MENU). Recompute now that both GAME.state and
    // ONLINE_SESSION reflect the match that just started, or the joiner would see
    // their own hand rendered on the wrong (far) side
    if let Some(canvas) = unsafe { crate::CANVAS.as_mut() } {
        canvas.resize();
    }
    crate::render::render::draw();
}

/// sends whatever clicks were buffered for the turn that just completed. A
/// no-op if the buffer is empty (eg. the turn that just ended was the remote
/// player's, whose clicks are never buffered locally -- see the access-control
/// guard in events/mousedown_handler.rs)
pub fn flush_outgoing_action() {
    let session = unsafe { ONLINE_SESSION.as_mut() };
    if let Some(session) = session {
        let clicks = session.take_outgoing();
        if !clicks.is_empty() {
            let clicks: Vec<String> = clicks.iter().map(|id| id.to_string()).collect();
            js_send_action(clicks);
        }
    }
}

/// parses a click id received over the network without panicking. Mirrors
/// `RenderId::from(String)` in src/render/util.rs, which panics on anything
/// it doesn't recognise (fine for DOM ids we generated ourselves, but not
/// safe for bytes that came off the network -- a malformed or corrupted
/// `move` message must be logged and ignored, never crash the tab, per the
/// design spec's error handling section)
fn parse_render_id(s: &str) -> Option<RenderId> {
    Some(match s {
        "p1=0" => RenderId::PlayerOne0,
        "p1=1" => RenderId::PlayerOne1,
        "p1=2" => RenderId::PlayerOne2,
        "p1=3" => RenderId::PlayerOne3,
        "p1=4" => RenderId::PlayerOne4,
        "p1=5" => RenderId::PlayerOne5,
        "p1=6" => RenderId::PlayerOne6,
        "p2=0" => RenderId::PlayerTwo0,
        "p2=1" => RenderId::PlayerTwo1,
        "p2=2" => RenderId::PlayerTwo2,
        "p2=3" => RenderId::PlayerTwo3,
        "p2=4" => RenderId::PlayerTwo4,
        "p2=5" => RenderId::PlayerTwo5,
        "p2=6" => RenderId::PlayerTwo6,
        "f=0" => RenderId::Field0,
        "f=1" => RenderId::Field1,
        "f=2" => RenderId::Field2,
        "f=3" => RenderId::Field3,
        "f=4" => RenderId::Field4,
        "f=5" => RenderId::Field5,
        "d=0" => RenderId::Deck,
        "d=1" => RenderId::Deal,
        "g=0" => RenderId::Graveyard0,
        "g=1" => RenderId::Graveyard1,
        "g=2" => RenderId::Graveyard2,
        "x=0" => RenderId::Cancel,
        "x=1" => RenderId::Multidone,
        "x=2" => RenderId::Confirm,
        "t=0" => RenderId::TurnIndicator,
        _ => return None,
    })
}

/// called from JS when a `move` message arrives: replays the peer's clicks
/// through the same turn-phase pipeline real clicks use, then auto-confirms if
/// that leaves the local side awaiting confirmation (eg. this browser's own
/// CONFIRM_BEFORE_PLAY setting is on) -- identical in spirit to how the AI's
/// moves are replayed and auto-confirmed in game/ai.rs. Any click id this
/// browser doesn't recognise is logged and skipped rather than applied --
/// both peers are trusted, but not infallible (a partially-lost message,
/// browser extension interference, etc. should never crash the tab)
#[wasm_bindgen]
pub fn on_action_received(clicks: Vec<String>) {
    let remote_player_num = match unsafe { ONLINE_SESSION.as_ref() } {
        Some(session) => {
            if session.local_player_num == 1 {
                2
            } else {
                1
            }
        }
        None => return,
    };

    for click in clicks {
        match parse_render_id(&click) {
            Some(id) => {
                crate::events::mousedown_handler::branch_turn_phase(id, remote_player_num);
            }
            None => {
                web_sys::console::log_1(&format!("ignoring unrecognised click id: {}", click).into());
            }
        }
    }

    let game = unsafe { GAME.as_ref() };
    if let Some(game) = game {
        if matches!(game.turn.phase, crate::game::structs::TurnPhase::CONFIRM) {
            crate::events::mousedown_handler::branch_turn_phase(
                RenderId::Confirm,
                remote_player_num,
            );
        }
    }
}
