# Online Multiplayer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let two players on different networks play a live 1-on-1 match: one creates a room and shares a code/link, the other joins, and moves are relayed peer-to-peer in real time.

**Architecture:** WebRTC peer-to-peer via the `@trystero-p2p/torrent` library (public-infra signaling, no server of ours, free TURN fallback for restrictive networks). The host generates the game once (deck shuffle + hands) and sends it whole to the joiner; every move after that is relayed as the sequence of `RenderId` clicks that make it up, replayed on the receiving side through the exact same `branch_turn_phase` pipeline already used to execute the AI's moves.

**Tech Stack:** Rust/WASM (existing), `@trystero-p2p/torrent` (new npm dependency), no new Rust crates (custom byte encoding instead of serde).

**Spec:** `docs/superpowers/specs/2026-08-25-online-multiplayer-design.md`

## Global Constraints

- No server or account of ours, ever — P2P only, signaling via Trystero's public-infra backends.
- TURN fallback uses the free, no-signup Open Relay static credentials (`openrelayproject`/`openrelayproject` at `openrelay.metered.ca`).
- Room joining supports both a manually-typed 6-character code and a `?room=CODE` link.
- Disconnect at any point ends the match immediately — no reconnection/resume (v1).
- `Card` is encoded as a single byte (0-20) for the one-time initial deck/hand transfer; no `serde` dependency.
- Every gameplay action is relayed as `Vec<RenderId>` clicks and replayed via `branch_turn_phase` — never full state resync.
- `RenderId::Confirm` and `RenderId::Cancel` are never part of the wire protocol (see spec's "Relaying moves").
- Existing `PLAYVS`/`PLAYAI` modes and their code are not modified except where explicitly listed below.

---

## Task 1: Card byte encoding

**Files:**
- Create: `src/game/card_encoding.rs`
- Modify: `src/game/mod.rs` (register module)
- Test: `tests/card_encoding.rs`

**Interfaces:**
- Produces: `card_to_byte(card: &Card) -> u8`, `byte_to_card(byte: u8) -> Option<Card>`, `cards_to_bytes(cards: &[Card]) -> Vec<u8>`, `bytes_to_cards(bytes: &[u8]) -> Option<Vec<Card>>` — all `pub`, used by Task 4.

- [ ] **Step 1: Write the failing test**

Create `tests/card_encoding.rs`:

```rust
use nabla_game;
use nabla_game::game::card_encoding::*;
use nabla_game::game::cards::*;

const ALL_CARDS: [Card; 21] = [
    Card::BasisCard(BasisCard::Zero),
    Card::BasisCard(BasisCard::One),
    Card::BasisCard(BasisCard::X),
    Card::BasisCard(BasisCard::X2),
    Card::BasisCard(BasisCard::Cos),
    Card::BasisCard(BasisCard::Sin),
    Card::BasisCard(BasisCard::E),
    Card::AlgebraicCard(AlgebraicCard::Div),
    Card::AlgebraicCard(AlgebraicCard::Mult),
    Card::AlgebraicCard(AlgebraicCard::Sqrt),
    Card::AlgebraicCard(AlgebraicCard::Inverse),
    Card::AlgebraicCard(AlgebraicCard::Log),
    Card::DerivativeCard(DerivativeCard::Derivative),
    Card::DerivativeCard(DerivativeCard::Integral),
    Card::DerivativeCard(DerivativeCard::Nabla),
    Card::DerivativeCard(DerivativeCard::Laplacian),
    Card::LimitCard(LimitCard::LimPosInf),
    Card::LimitCard(LimitCard::LimNegInf),
    Card::LimitCard(LimitCard::Lim0),
    Card::LimitCard(LimitCard::Liminf),
    Card::LimitCard(LimitCard::Limsup),
];

#[test]
fn test_every_card_round_trips_through_a_byte() {
    for card in ALL_CARDS.iter() {
        let byte = card_to_byte(card);
        assert_eq!(byte_to_card(byte), Some(*card), "byte {} did not round-trip", byte);
    }
}

#[test]
fn test_bytes_are_unique() {
    let mut bytes: Vec<u8> = ALL_CARDS.iter().map(card_to_byte).collect();
    bytes.sort();
    bytes.dedup();
    assert_eq!(bytes.len(), ALL_CARDS.len(), "two cards mapped to the same byte");
}

#[test]
fn test_unknown_byte_returns_none() {
    assert_eq!(byte_to_card(255), None);
}

#[test]
fn test_cards_to_bytes_and_back() {
    let hand = vec![
        Card::BasisCard(BasisCard::X),
        Card::DerivativeCard(DerivativeCard::Nabla),
        Card::LimitCard(LimitCard::Limsup),
    ];
    let bytes = cards_to_bytes(&hand);
    assert_eq!(bytes_to_cards(&bytes), Some(hand));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test card_encoding`
Expected: FAIL to compile — `card_encoding` module does not exist yet.

- [ ] **Step 3: Write the implementation**

Create `src/game/card_encoding.rs`:

```rust
// outer crate imports
use super::cards::*;

/// maps every Card variant to a stable single byte, and back. Used only for the
/// one-time transfer of the shuffled deck + starting hands when an online match
/// begins (see game/online.rs) -- every move after that is relayed as clicks,
/// not cards, so this never needs to handle anything but the 21 card kinds.
pub fn card_to_byte(card: &Card) -> u8 {
    match card {
        Card::BasisCard(BasisCard::Zero) => 0,
        Card::BasisCard(BasisCard::One) => 1,
        Card::BasisCard(BasisCard::X) => 2,
        Card::BasisCard(BasisCard::X2) => 3,
        Card::BasisCard(BasisCard::Cos) => 4,
        Card::BasisCard(BasisCard::Sin) => 5,
        Card::BasisCard(BasisCard::E) => 6,
        Card::AlgebraicCard(AlgebraicCard::Div) => 7,
        Card::AlgebraicCard(AlgebraicCard::Mult) => 8,
        Card::AlgebraicCard(AlgebraicCard::Sqrt) => 9,
        Card::AlgebraicCard(AlgebraicCard::Inverse) => 10,
        Card::AlgebraicCard(AlgebraicCard::Log) => 11,
        Card::DerivativeCard(DerivativeCard::Derivative) => 12,
        Card::DerivativeCard(DerivativeCard::Integral) => 13,
        Card::DerivativeCard(DerivativeCard::Nabla) => 14,
        Card::DerivativeCard(DerivativeCard::Laplacian) => 15,
        Card::LimitCard(LimitCard::LimPosInf) => 16,
        Card::LimitCard(LimitCard::LimNegInf) => 17,
        Card::LimitCard(LimitCard::Lim0) => 18,
        Card::LimitCard(LimitCard::Liminf) => 19,
        Card::LimitCard(LimitCard::Limsup) => 20,
    }
}

pub fn byte_to_card(byte: u8) -> Option<Card> {
    match byte {
        0 => Some(Card::BasisCard(BasisCard::Zero)),
        1 => Some(Card::BasisCard(BasisCard::One)),
        2 => Some(Card::BasisCard(BasisCard::X)),
        3 => Some(Card::BasisCard(BasisCard::X2)),
        4 => Some(Card::BasisCard(BasisCard::Cos)),
        5 => Some(Card::BasisCard(BasisCard::Sin)),
        6 => Some(Card::BasisCard(BasisCard::E)),
        7 => Some(Card::AlgebraicCard(AlgebraicCard::Div)),
        8 => Some(Card::AlgebraicCard(AlgebraicCard::Mult)),
        9 => Some(Card::AlgebraicCard(AlgebraicCard::Sqrt)),
        10 => Some(Card::AlgebraicCard(AlgebraicCard::Inverse)),
        11 => Some(Card::AlgebraicCard(AlgebraicCard::Log)),
        12 => Some(Card::DerivativeCard(DerivativeCard::Derivative)),
        13 => Some(Card::DerivativeCard(DerivativeCard::Integral)),
        14 => Some(Card::DerivativeCard(DerivativeCard::Nabla)),
        15 => Some(Card::DerivativeCard(DerivativeCard::Laplacian)),
        16 => Some(Card::LimitCard(LimitCard::LimPosInf)),
        17 => Some(Card::LimitCard(LimitCard::LimNegInf)),
        18 => Some(Card::LimitCard(LimitCard::Lim0)),
        19 => Some(Card::LimitCard(LimitCard::Liminf)),
        20 => Some(Card::LimitCard(LimitCard::Limsup)),
        _ => None,
    }
}

pub fn cards_to_bytes(cards: &[Card]) -> Vec<u8> {
    cards.iter().map(card_to_byte).collect()
}

/// returns None if any byte is unrecognised (eg. corrupted/foreign message)
pub fn bytes_to_cards(bytes: &[u8]) -> Option<Vec<Card>> {
    bytes.iter().map(|b| byte_to_card(*b)).collect()
}
```

Modify `src/game/mod.rs` — add the module:

```rust
pub mod ai;
pub mod card_counts;
pub mod card_encoding;
pub mod cards;
pub mod field;
pub mod flags;
pub mod structs;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test card_encoding`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add src/game/card_encoding.rs src/game/mod.rs tests/card_encoding.rs
git commit -m "Add Card <-> byte encoding for online match setup"
```

---

## Task 2: OnlineSession state + PLAYONLINE game state

**Files:**
- Create: `src/game/online.rs` (state/logic only in this task — no wasm-bindgen wiring yet, that's Task 3)
- Modify: `src/game/mod.rs` (register module)
- Modify: `src/game/structs.rs` (add `GameState::PLAYONLINE`)
- Modify: `src/lib.rs:14` (`mod render;` -> `pub mod render;`, needed so `tests/online.rs` can reach `RenderId`)
- Test: `tests/online.rs`

**Interfaces:**
- Consumes: `RenderId` (from `crate::render::util`, already `Eq + PartialEq + Debug + Copy + Clone`).
- Produces: `OnlineSession::new(local_player_num: u32) -> OnlineSession`, `OnlineSession::record_click(&mut self, id: RenderId)`, `OnlineSession::take_outgoing(&mut self) -> Vec<RenderId>`, `pub static mut ONLINE_SESSION: Option<OnlineSession>`. `GameState::PLAYONLINE` variant + `"PLAYONLINE"` string mapping. Used by Tasks 3, 5, 6.

- [ ] **Step 1: Write the failing test**

Create `tests/online.rs`:

```rust
use nabla_game;
use nabla_game::game::online::OnlineSession;
use nabla_game::render::util::RenderId;

#[test]
fn test_record_click_buffers_normal_clicks_in_order() {
    let mut session = OnlineSession::new(1);
    session.record_click(RenderId::Field0);
    session.record_click(RenderId::PlayerOne3);
    assert_eq!(session.take_outgoing(), vec![RenderId::Field0, RenderId::PlayerOne3]);
}

#[test]
fn test_record_click_excludes_confirm() {
    let mut session = OnlineSession::new(1);
    session.record_click(RenderId::Field0);
    session.record_click(RenderId::Confirm);
    assert_eq!(session.take_outgoing(), vec![RenderId::Field0]);
}

#[test]
fn test_record_click_cancel_discards_the_whole_buffer() {
    let mut session = OnlineSession::new(1);
    session.record_click(RenderId::Field0);
    session.record_click(RenderId::PlayerOne1);
    session.record_click(RenderId::Cancel);
    assert_eq!(session.take_outgoing(), Vec::<RenderId>::new());
}

#[test]
fn test_take_outgoing_clears_the_buffer() {
    let mut session = OnlineSession::new(1);
    session.record_click(RenderId::Field0);
    session.take_outgoing();
    assert_eq!(session.take_outgoing(), Vec::<RenderId>::new());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test online`
Expected: FAIL to compile — `online` module / `OnlineSession` do not exist yet.

- [ ] **Step 3: Write the implementation**

Modify `src/lib.rs` — change line 14 from `mod render;` to:

```rust
pub mod render;
```

Modify `src/game/structs.rs` — add the variant to the `GameState` enum and its `From<&str>` impl:

```rust
/// different possible states of game and UI
#[derive(Debug)]
pub enum GameState {
    MENU,
    PLAYAI,
    PLAYVS,
    PLAYONLINE,
    TUTORIAL,
    SETTINGS,
    CREDITS,
}

impl From<&str> for GameState {
    fn from(input: &str) -> Self {
        match input {
            "MENU" => Self::MENU,
            "PLAYAI" => Self::PLAYAI,
            "PLAYVS" => Self::PLAYVS,
            "PLAYONLINE" => Self::PLAYONLINE,
            "TUTORIAL" => Self::TUTORIAL,
            "SETTINGS" => Self::SETTINGS,
            "CREDITS" => Self::CREDITS,
            _ => unreachable!("{} is not a valid GameState", input),
        }
    }
}
```

Do **not** add `PLAYONLINE` to `set_state`'s `PLAYAI | PLAYVS` arm — leave `set_state` itself unmodified. This is deliberate: unlike `PLAYAI`/`PLAYVS`, where clicking the menu button *is* the start of the game, clicking "Play Online" only opens the create/join panel — the menu must stay open and the canvas must stay hidden until a peer actually connects and the match is synced. `PLAYONLINE` falls into `set_state`'s existing `_ => {}` arm, exactly like `SETTINGS`/`TUTORIAL`/`CREDITS` do — clicking the menu button sets `game.state = GameState::PLAYONLINE` and reveals the `menu-PLAYONLINE` panel (Task 6 wires this through the same generic mechanism Settings already uses), without touching the menu's visibility or the canvas. The close-and-draw that actually starts the match happens later, explicitly, in Task 4's `start_online_game` — see that task for why.

Create `src/game/online.rs`:

```rust
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
```

Modify `src/game/mod.rs`:

```rust
pub mod ai;
pub mod card_counts;
pub mod card_encoding;
pub mod cards;
pub mod field;
pub mod flags;
pub mod online;
pub mod structs;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test online`
Expected: PASS (4 tests)

Also run the full suite to confirm nothing else broke from the `pub mod render` visibility change:

Run: `cargo test --no-fail-fast 2>&1 | grep -E "test result|FAILED"`
Expected: same pass/fail counts as before this task (one pre-existing unrelated failure, `test_complex_special_coefficients`, is expected and not something this plan touches).

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/game/structs.rs src/game/online.rs src/game/mod.rs tests/online.rs
git commit -m "Add OnlineSession state and GameState::PLAYONLINE"
```

---

## Task 3: Trystero connection (room create/join, connect/disconnect events)

**Files:**
- Modify: `package.json` (add dependency), `yarn.lock` (regenerated by `yarn install`)
- Create: `js/online.js`
- Modify: `js/index.js` (wire the wasm module reference into online.js)
- Modify: `src/game/online.rs` (wasm-bindgen extern block + exported callbacks)
- Test: manual, via two Playwright browser contexts (no automated Rust test — this task is entirely browser/WebRTC glue)

**Interfaces:**
- Consumes: `OnlineSession` (Task 2).
- Produces (Rust, `#[wasm_bindgen]`, callable from JS): `on_peer_connected()`, `on_peer_disconnected()`, `on_connection_error()`. Produces (JS, callable from Rust via `extern "C"`): `js_create_room() -> String`, `js_join_room(code: String)`, `js_leave_room()`. Both sides used starting Task 4.

- [ ] **Step 1: Add the dependency**

```bash
yarn add "@trystero-p2p/torrent"
```

This updates `package.json` and `yarn.lock`.

- [ ] **Step 2: Write `js/online.js`**

```js
import { joinRoom } from '@trystero-p2p/torrent';

// identifies this app in Trystero's public signaling namespace -- not a secret,
// just keeps our rooms from colliding with unrelated apps using the same relays
const APP_ID = 'nabla-game-naoto-programming';

// Open Relay Project's free, no-signup TURN fallback (used only when a direct
// P2P connection can't be established, eg. restrictive NATs)
const TURN_CONFIG = [
	{
		urls: [
			'turn:openrelay.metered.ca:80',
			'turn:openrelay.metered.ca:443',
			'turn:openrelay.metered.ca:443?transport=tcp',
		],
		username: 'openrelayproject',
		credential: 'openrelayproject',
	},
];

const CODE_CHARS = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'; // no 0/O/1/I/L

const generateRoomCode = () =>
	Array.from({ length: 6 }, () => CODE_CHARS[Math.floor(Math.random() * CODE_CHARS.length)]).join('');

let room = null;
let initAction = null;
let moveAction = null;
let wasm = null;

// called once from js/index.js after the wasm module has loaded
export const setWasm = wasmModule => {
	wasm = wasmModule;
};

const attachRoomListeners = () => {
	room.onPeerJoin = () => wasm && wasm.on_peer_connected();
	room.onPeerLeave = () => wasm && wasm.on_peer_disconnected();

	initAction = room.makeAction('init');
	moveAction = room.makeAction('move');

	initAction.onMessage = data => {
		wasm && wasm.on_init_received(data.deck, data.hand1, data.hand2);
	};
	moveAction.onMessage = data => {
		wasm && wasm.on_action_received(data.clicks);
	};
};

export const js_create_room = () => {
	const code = generateRoomCode();
	room = joinRoom({ appId: APP_ID, turnConfig: TURN_CONFIG }, code, {
		onJoinError: () => wasm && wasm.on_connection_error(),
	});
	attachRoomListeners();
	return code;
};

export const js_join_room = code => {
	// room codes are always generated uppercase (see CODE_CHARS above); normalize
	// a manually-typed code so a stray lowercase paste doesn't silently join a
	// different (nonexistent) room and only fail 30s later via the connect timeout
	const normalizedCode = code.trim().toUpperCase();
	room = joinRoom({ appId: APP_ID, turnConfig: TURN_CONFIG }, normalizedCode, {
		onJoinError: () => wasm && wasm.on_connection_error(),
	});
	attachRoomListeners();
};

export const js_send_init = (deck, hand1, hand2) => {
	initAction.send({ deck: Array.from(deck), hand1: Array.from(hand1), hand2: Array.from(hand2) });
};

export const js_send_action = clicks => {
	moveAction.send({ clicks });
};

export const js_leave_room = () => {
	if (room) room.leave();
	room = null;
	initAction = null;
	moveAction = null;
};

export const getRoomCodeFromUrl = () => new URLSearchParams(window.location.search).get('room');
```

- [ ] **Step 3: Wire the wasm module reference in `js/index.js`**

Replace the contents of `js/index.js`:

```js
import { initI18n } from './i18n.js';
import { setWasm } from './online.js';

import('./katex.js');
import('../pkg/index.js').then(setWasm).catch(console.error);

initI18n();
```

- [ ] **Step 4: Add the Rust side of the boundary**

Append to `src/game/online.rs` (after the `OnlineSession` impl block):

```rust
// wasm-bindgen imports
use wasm_bindgen::prelude::*;
// root imports
use crate::{GAME, MENU};

#[wasm_bindgen(module = "/js/online.js")]
extern "C" {
    #[wasm_bindgen(js_name = js_create_room)]
    fn js_create_room() -> String;
    #[wasm_bindgen(js_name = js_join_room)]
    fn js_join_room(code: String);
    #[wasm_bindgen(js_name = js_leave_room)]
    fn js_leave_room();
}

/// starts hosting a room, returning the code to show the player
pub fn create_room() -> String {
    let code = js_create_room();
    unsafe { ONLINE_SESSION = Some(OnlineSession::new(1)) };
    code
}

/// attempts to join an existing room by its code
pub fn join_room(code: String) {
    js_join_room(code);
    unsafe { ONLINE_SESSION = Some(OnlineSession::new(2)) };
}

pub fn leave_room() {
    js_leave_room();
    unsafe { ONLINE_SESSION = None };
}

/// called from JS when the peer connects. The host (player 1) sends the
/// initial game state once connected; the joiner (player 2) just waits for it
/// (see Task 4)
#[wasm_bindgen]
pub fn on_peer_connected() {
    let session = unsafe { ONLINE_SESSION.as_mut() };
    if let Some(session) = session {
        session.connected = true;
    }
}

/// called from JS when the peer disconnects, at any point (see Task 7 for the
/// full user-facing handling; this task only updates the session flag)
#[wasm_bindgen]
pub fn on_peer_disconnected() {
    let session = unsafe { ONLINE_SESSION.as_mut() };
    if let Some(session) = session {
        session.connected = false;
    }
}

/// called from JS when Trystero reports the room could not be joined at all
/// (see Task 7 for the user-facing timeout/error handling)
#[wasm_bindgen]
pub fn on_connection_error() {}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check --lib`
Expected: no errors. (This checks the Rust side only; `js_create_room`/`js_join_room`/`js_leave_room` are declared but not yet called from anywhere outside this module, which is fine — `create_room`/`join_room`/`leave_room` are `pub` for Task 6's UI wiring to call.)

- [ ] **Step 6: Verify the JS side loads without errors**

Run `yarn install` then `yarn build`, confirm it completes without error (this doesn't yet exercise the connection — that needs the UI from Task 6 — but confirms the new module bundles cleanly):

```bash
yarn install
yarn build
```

Expected: build succeeds, no webpack errors about `@trystero-p2p/torrent` or `js/online.js`.

- [ ] **Step 7: Commit**

```bash
git add package.json yarn.lock js/online.js js/index.js src/game/online.rs
git commit -m "Add Trystero room create/join and connect/disconnect wiring"
```

---

## Task 4: Initial game state sync

**Files:**
- Modify: `src/game/online.rs`
- Modify: `js/online.js` (none needed — `js_send_init`/`onMessage` already exist from Task 3)

**Interfaces:**
- Consumes: `cards_to_bytes`/`bytes_to_cards` (Task 1), `Game::new()`/`Field::new()` (existing), `card_counts::get_new_deck` (existing).
- Produces: host sends `init` after connecting; both sides end up with `GAME` set to the same match. Used by Task 5 (which needs a synced `Game` to relay moves against) and Task 6 (UI transitions to the game screen once this completes).

- [ ] **Step 1: Implement the host's send-on-connect and the joiner's receive**

Modify `src/game/online.rs`'s `on_peer_connected` and add the receive handler. Replace the `on_peer_connected` function with:

```rust
/// called from JS when the peer connects. The host (player 1) generates the
/// match now and sends it whole; the joiner (player 2) just waits for `init`
#[wasm_bindgen]
pub fn on_peer_connected() {
    let session = unsafe { ONLINE_SESSION.as_mut() };
    let is_host = match &session {
        Some(session) => {
            session.connected = true;
            session.local_player_num == 1
        }
        None => return,
    };

    if is_host {
        let game = crate::game::structs::Game::new();
        let deck = crate::game::card_encoding::cards_to_bytes(&game.deck);
        let hand1 = crate::game::card_encoding::cards_to_bytes(&game.player_1);
        let hand2 = crate::game::card_encoding::cards_to_bytes(&game.player_2);
        js_send_init(deck, hand1, hand2);
        start_online_game(game);
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
/// panel -- see Task 2), this is the point that closes the menu and draws the
/// board. `game.state` is set directly rather than via `Game::set_state`,
/// since `set_state` intentionally does *not* treat `PLAYONLINE` as a
/// close-and-draw trigger (that would fire prematurely on the button click,
/// before any connection exists)
fn start_online_game(mut game: crate::game::structs::Game) {
    use crate::game::structs::GameState;
    game.state = GameState::PLAYONLINE;
    unsafe { GAME = Some(game) };
    if let Some(menu) = unsafe { MENU.as_ref() } {
        menu.close();
    }
    crate::render::render::draw();
}
```

- [ ] **Step 2: Add the constructor `Game::from_online_parts` used above**

Modify `src/game/structs.rs` — add this method inside `impl Game` (after `pub fn new()`):

```rust
    /// builds a Game from deck/hand contents received from the host, instead
    /// of generating them locally -- both sides of an online match must end
    /// up with byte-for-byte the same deck and hands, so only the host's
    /// `Game::new()` ever calls the RNG for a given match (see game/online.rs)
    pub fn from_online_parts(deck: Vec<Card>, player_1: Vec<Card>, player_2: Vec<Card>) -> Game {
        Game {
            state: GameState::MENU,
            turn: Turn {
                number: 0,
                phase: TurnPhase::IDLE,
            },
            field: Field::new(),
            player_1,
            player_2,
            deck,
            graveyard: vec![],
            active: ActiveCards {
                selected: Vec::default(),
                hover: None,
            },
            pending: None,
        }
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --lib`
Expected: no errors.

- [ ] **Step 4: Verify with two Playwright browser contexts**

This can't be checked with `cargo test` (it needs two live WebRTC peers). Write a throwaway script in the scratchpad directory, run it against the deployed Pages URL once Task 6 (UI) exists — note this verification step in Task 6 instead, since there's no UI yet to trigger `create_room`/`join_room` from. **Skip live verification for this task specifically; it will be verified together with Task 6's UI as part of that task's Playwright check**, which confirms both sides render the identical field/hands after connecting.

- [ ] **Step 5: Commit**

```bash
git add src/game/online.rs src/game/structs.rs
git commit -m "Sync initial deck and hands when an online match connects"
```

---

## Task 5: Move relay (action buffering, sending, and replay)

**Files:**
- Modify: `src/events/mousedown_handler.rs`
- Modify: `src/game/online.rs`

**Interfaces:**
- Consumes: `OnlineSession::record_click`/`take_outgoing` (Task 2), `branch_turn_phase`/`next_turn` (existing, already `pub`).
- Produces: `on_action_received(clicks: Vec<String>)` (Rust, called from JS). Local clicks during the online player's own turn get buffered and sent; the access-control guard extends to online mode. Used by Task 6/7 for full end-to-end play.

- [ ] **Step 1: Extend the access-control guard in `handle_mousedown`**

Modify `src/events/mousedown_handler.rs`. The existing PLAYAI guard is:

```rust
    // in a PLAYAI game, the AI's turn belongs to the AI alone -- without this, a click
    // on the canvas during the AI's turn is dispatched exactly like a real player 2
    // move (turn phase routing only looks at whose turn number it is, not who is
    // actually meant to be playing it), letting a human take over mid-game
    if matches!(game.state, GameState::PLAYAI)
        && game.get_current_player_num() == crate::game::ai::AI_PLAYER_NUM
    {
        return;
    }
```

Replace it with (adding the online-mode case directly below):

```rust
    // in a PLAYAI game, the AI's turn belongs to the AI alone -- without this, a click
    // on the canvas during the AI's turn is dispatched exactly like a real player 2
    // move (turn phase routing only looks at whose turn number it is, not who is
    // actually meant to be playing it), letting a human take over mid-game
    if matches!(game.state, GameState::PLAYAI)
        && game.get_current_player_num() == crate::game::ai::AI_PLAYER_NUM
    {
        return;
    }

    // in a PLAYONLINE game, the local browser may only act on its own turns --
    // the remote player's turns are driven exclusively by replaying messages
    // received over the network (see on_action_received in game/online.rs)
    if matches!(game.state, GameState::PLAYONLINE) {
        let local_player_num = unsafe { crate::game::online::ONLINE_SESSION.as_ref() }
            .map(|session| session.local_player_num);
        if local_player_num != Some(game.get_current_player_num()) {
            return;
        }
    }
```

- [ ] **Step 2: Buffer local clicks and flush on turn completion**

In the same file, modify `handle_mousedown` to record the click right before dispatching (after the two guards above, right before the `match turn { ... }` block):

```rust
    // record this click for relay if it's part of the local player's own
    // online turn (the guard above already filtered out anything else)
    if matches!(game.state, GameState::PLAYONLINE) {
        if let Some(session) = unsafe { crate::game::online::ONLINE_SESSION.as_mut() } {
            session.record_click(id);
        }
    }

    match turn {
```

Modify `next_turn()` (same file) to flush the buffer right after the turn actually completes. Current end of the function:

```rust
    let field = game.field.basis.iter();
    if field.clone().take(3).all(|f| f.basis.is_none()) {
        // player 1 wins
        game.game_over(1);
    } else if field.clone().skip(3).all(|f| f.basis.is_none()) {
        // player 2 wins
        game.game_over(2);
    } else {
        crate::game::ai::maybe_take_ai_turn();
    }
}
```

Replace with:

```rust
    if matches!(game.state, GameState::PLAYONLINE) {
        crate::game::online::flush_outgoing_action();
    }

    let field = game.field.basis.iter();
    if field.clone().take(3).all(|f| f.basis.is_none()) {
        // player 1 wins
        game.game_over(1);
    } else if field.clone().skip(3).all(|f| f.basis.is_none()) {
        // player 2 wins
        game.game_over(2);
    } else {
        crate::game::ai::maybe_take_ai_turn();
    }
}
```

- [ ] **Step 3: Add `flush_outgoing_action`, `js_send_action` binding, and `on_action_received` to `game/online.rs`**

Add to the `extern "C"` block in `src/game/online.rs`:

```rust
    #[wasm_bindgen(js_name = js_send_action)]
    fn js_send_action(clicks: Vec<String>);
```

Add these functions (near the other `#[wasm_bindgen]` functions):

```rust
/// sends whatever clicks were buffered for the turn that just completed. A
/// no-op if the buffer is empty (eg. the turn that just ended was the remote
/// player's, whose clicks are never buffered locally -- see the access-control
/// guard in handle_mousedown)
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
fn parse_render_id(s: &str) -> Option<crate::render::util::RenderId> {
    use crate::render::util::RenderId;
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
        if matches!(
            game.turn.phase,
            crate::game::structs::TurnPhase::CONFIRM
        ) {
            crate::events::mousedown_handler::branch_turn_phase(
                crate::render::util::RenderId::Confirm,
                remote_player_num,
            );
        }
    }
}
```

(`web_sys::console::log_1` above needs the `"console"` web-sys feature — already enabled in `Cargo.toml:47`, no change needed.)

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --lib`
Expected: no errors.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "test result|FAILED"`
Expected: same results as Task 2's step 4 (one pre-existing unrelated failure only).

- [ ] **Step 6: Commit**

```bash
git add src/events/mousedown_handler.rs src/game/online.rs
git commit -m "Relay and replay moves for online matches"
```

---

## Task 6: UI — menu, create/join panel, room code/link, i18n

**Files:**
- Modify: `static/index.html`
- Modify: `static/index.css`
- Modify: `js/i18n.js`
- Modify: `src/menu.rs`
- Modify: `js/online.js` (URL param check on load)

**Interfaces:**
- Consumes: `online::create_room() -> String`, `online::join_room(code: String)`, `online::leave_room()` (Task 3), `getRoomCodeFromUrl()` (Task 3).
- Produces: a usable end-to-end flow. This is the task whose Playwright verification exercises Tasks 3-5 together for the first time.

- [ ] **Step 1: Add the HTML**

Modify `static/index.html`. Add the menu button inside `<div id="menu-MENU" class="menu-item">`, after the `button-PLAYAI` line:

```html
					<button class="menu-button" id="button-PLAYONLINE" data-i18n="menu.playonline">Play Online</button>
```

Add a new panel as a sibling of `menu-SETTINGS`/`menu-TUTORIAL` (place it right after the closing `</div>` of `menu-CARDCOUNTS`, before `menu-TUTORIAL`):

```html
				<div id="menu-PLAYONLINE" class="menu-item" hidden>
					<div id="online-choice">
						<button class="menu-button" id="button-ONLINE_CREATE" data-i18n="online.create">
							Create Game
						</button>
						<button class="menu-button" id="button-ONLINE_JOIN_SHOW" data-i18n="online.join">
							Join Game
						</button>
					</div>
					<div id="online-create-panel" hidden>
						<h3 data-i18n="online.roomCode">Room Code</h3>
						<p id="online-room-code"></p>
						<button class="menu-button" id="button-COPY_CODE" data-i18n="online.copyCode">
							Copy Code
						</button>
						<button class="menu-button" id="button-COPY_LINK" data-i18n="online.copyLink">
							Copy Link
						</button>
					</div>
					<div id="online-join-panel" hidden>
						<input id="online-join-code-input" type="text" maxlength="6" />
						<button
							class="menu-button"
							id="button-ONLINE_JOIN_CONNECT"
							data-i18n="online.connect"
						>
							Connect
						</button>
					</div>
					<p id="online-status"></p>
				</div>
```

- [ ] **Step 2: Add CSS for the new panel**

Modify `static/index.css` — append:

```css
#online-choice {
	display: flex;
	gap: 1em;
	justify-content: center;
}
#online-room-code {
	font-size: 2rem;
	font-weight: bold;
	letter-spacing: 0.2em;
	text-align: center;
}
#online-join-code-input {
	display: block;
	margin: 1em auto;
	font-size: 1.5rem;
	letter-spacing: 0.2em;
	text-align: center;
	width: 8rem;
	background-color: var(--color-light);
	color: var(--color-dark);
	border-width: 3px;
	border-style: solid;
	border-color: var(--color-dark);
	border-radius: 5px;
	padding: 0.25rem;
}
#online-status {
	text-align: center;
	margin-top: 1em;
}
```

- [ ] **Step 3: Add i18n strings**

Modify `js/i18n.js`. In the `en` object, add after `'menu.playai': 'Play vs AI',`:

```js
		'menu.playonline': 'Play Online',
		'online.create': 'Create Game',
		'online.join': 'Join Game',
		'online.roomCode': 'Room Code',
		'online.copyCode': 'Copy Code',
		'online.copyLink': 'Copy Link',
		'online.connect': 'Connect',
```

In the `ja` object, add after `'menu.playai': 'AI対戦',`:

```js
		'menu.playonline': 'オンライン対戦',
		'online.create': '対戦を作成',
		'online.join': '対戦に参加',
		'online.roomCode': 'ルームコード',
		'online.copyCode': 'コードをコピー',
		'online.copyLink': 'リンクをコピー',
		'online.connect': '接続',
```

(Status text is set dynamically from Rust, not via `data-i18n` — Task 7 covers those strings when the states they describe are introduced.)

- [ ] **Step 4: Wire up the buttons and URL param in `src/menu.rs`**

Add a new struct and wire it up. Modify the imports at the top of `src/menu.rs`:

```rust
use crate::game::online;
```

Add a new struct after `SettingsMenu`'s `impl` block closes:

```rust
/// controller for the "Play Online" create/join panel
#[allow(dead_code)]
pub struct OnlineMenu {
    create_button: Element,
    create_listener: EventListener,
    join_show_button: Element,
    join_show_listener: EventListener,
    join_connect_button: Element,
    join_connect_listener: EventListener,
    copy_code_button: Element,
    copy_code_listener: EventListener,
    copy_link_button: Element,
    copy_link_listener: EventListener,
}

impl OnlineMenu {
    pub fn new(document: &Document) -> Self {
        let create_panel = document.get_element_by_id("online-create-panel").unwrap();
        let join_panel = document.get_element_by_id("online-join-panel").unwrap();
        let room_code_display = document.get_element_by_id("online-room-code").unwrap();
        let status = document.get_element_by_id("online-status").unwrap();
        let join_input = document
            .get_element_by_id("online-join-code-input")
            .unwrap();

        let create_button = document.get_element_by_id("button-ONLINE_CREATE").unwrap();
        let create_listener = {
            let create_panel = create_panel.clone();
            let room_code_display = room_code_display.clone();
            let status = status.clone();
            EventListener::new(&create_button, "click", move |_e| {
                let code = online::create_room();
                room_code_display.set_text_content(Some(code.as_str()));
                create_panel.remove_attribute("hidden").ok();
                status.set_text_content(Some("Waiting for opponent..."));
            })
        };

        let join_show_button = document
            .get_element_by_id("button-ONLINE_JOIN_SHOW")
            .unwrap();
        let join_show_listener = {
            let join_panel = join_panel.clone();
            EventListener::new(&join_show_button, "click", move |_e| {
                join_panel.remove_attribute("hidden").ok();
            })
        };

        let join_connect_button = document
            .get_element_by_id("button-ONLINE_JOIN_CONNECT")
            .unwrap();
        let join_connect_listener = {
            let join_input = join_input.clone();
            let status = status.clone();
            EventListener::new(&join_connect_button, "click", move |_e| {
                let code = join_input
                    .dyn_ref::<HtmlInputElement>()
                    .unwrap()
                    .value();
                online::join_room(code);
                status.set_text_content(Some("Connecting..."));
            })
        };

        let copy_code_button = document.get_element_by_id("button-COPY_CODE").unwrap();
        let copy_code_listener = {
            let room_code_display = room_code_display.clone();
            EventListener::new(&copy_code_button, "click", move |_e| {
                if let Some(code) = room_code_display.text_content() {
                    online::copy_to_clipboard(code);
                }
            })
        };

        let copy_link_button = document.get_element_by_id("button-COPY_LINK").unwrap();
        let copy_link_listener = {
            let room_code_display = room_code_display.clone();
            EventListener::new(&copy_link_button, "click", move |_e| {
                if let Some(code) = room_code_display.text_content() {
                    let location = web_sys::window().unwrap().location();
                    let url = format!(
                        "{}{}?room={}",
                        location.origin().unwrap(),
                        location.pathname().unwrap(),
                        code
                    );
                    online::copy_to_clipboard(url);
                }
            })
        };

        // if the page was opened via a ?room=CODE link, jump straight to the
        // join panel with the code pre-filled
        if let Some(code) = online::room_code_from_url() {
            join_input
                .dyn_ref::<HtmlInputElement>()
                .unwrap()
                .set_value(code.as_str());
            join_panel.remove_attribute("hidden").ok();
        }

        Self {
            create_button,
            create_listener,
            join_show_button,
            join_show_listener,
            join_connect_button,
            join_connect_listener,
            copy_code_button,
            copy_code_listener,
            copy_link_button,
            copy_link_listener,
        }
    }
}
```

Modify `Menu::new` (top of the file) to construct it and store it, and modify the main-menu button click handler to open the PLAYONLINE panel like Settings does (it already does this generically via `menu.menu_children.contains_key(target_state)` — since `menu-PLAYONLINE` follows the same `id="menu-ID"` convention as the other panels, **no change is needed there**; it's picked up automatically by the existing scan in `Menu::new`).

Add the field to the `Menu` struct and construct it:

```rust
pub struct Menu {
    pub menu_children: HashMap<String, Element>,
    pub menu_element: Element,

    pub main_menu_button: Element,
    pub main_menu_listener: EventListener,
    pub game_over_menu: Element,
    pub game_over_listener: EventListener,

    pub main_menu: MainMenu,
    pub settings_menu: SettingsMenu,
    pub online_menu: OnlineMenu,
}
```

In `Menu::new`, after `let settings_menu = SettingsMenu::new(document);`:

```rust
        let online_menu = OnlineMenu::new(document);
```

And in the final `Menu { ... }` construction, add `online_menu,`.

Also modify `MainMenu::new`'s button id list to include the new button:

```rust
        let button_elements: Vec<Element> =
            vec!["PLAYVS", "PLAYAI", "PLAYONLINE", "TUTORIAL", "SETTINGS", "CREDITS"]
```

- [ ] **Step 5: Add `copy_to_clipboard` and `room_code_from_url` to `game/online.rs`**

Add to the `extern "C"` block:

```rust
    #[wasm_bindgen(js_name = getRoomCodeFromUrl)]
    fn get_room_code_from_url() -> Option<String>;
```

Add a new function for clipboard access (needs `web-sys`'s Clipboard API — add `"Clipboard"` and `"Navigator"` to the `web-sys` features list in `Cargo.toml`, next to the existing `"Window"` entry):

```rust
/// copies `text` to the clipboard; errors (eg. permission denied) are ignored --
/// this is a "nice to have" convenience button, not a critical action
pub fn copy_to_clipboard(text: String) {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().clipboard().write_text(text.as_str());
    }
}

pub fn room_code_from_url() -> Option<String> {
    get_room_code_from_url()
}
```

Modify `Cargo.toml`'s `[dependencies.web-sys]` `features` list to add the two new entries next to `"Window",`:

```toml
  "Window",
  "Navigator",
  "Clipboard",
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check --lib`
Expected: no errors. If `Clipboard`/`Navigator` types don't expose `write_text` with this exact signature, check the currently-pinned `web-sys` version's docs (`Cargo.lock`'s `web-sys` version) and adjust — this is the one spot in this plan where the exact method signature wasn't independently verified against the pinned dependency version, so confirm it compiles before moving on.

- [ ] **Step 7: Build and verify with two Playwright browser contexts**

```bash
yarn install
yarn build
```

Then push to a branch/deploy or run the dev server, and use two separate Playwright browser contexts (as done throughout this project's prior sessions) against the same URL:
1. Context A: open menu -> Play Online -> Create Game. Read the displayed room code.
2. Context B: open menu -> Play Online -> Join Game -> type the code -> Connect.
3. Wait ~2s for the WebRTC handshake.
4. Screenshot both contexts: both should show the game screen (not the menu) with identical field cards (`1, x, x²` on both sides) and different (but both 7-card) hands.

- [ ] **Step 8: Commit**

```bash
git add static/index.html static/index.css js/i18n.js js/online.js src/menu.rs src/game/online.rs Cargo.toml
git commit -m "Add Play Online menu, create/join panel, and clipboard/URL helpers"
```

---

## Task 7: Disconnect handling and connection timeout

**Files:**
- Modify: `src/game/online.rs`
- Modify: `js/online.js`
- Modify: `js/i18n.js`

**Interfaces:**
- Consumes: `on_peer_disconnected` (Task 3, extends it), `on_connection_error` (Task 3, extends it).
- Produces: user-visible disconnect notice + return to menu; a 30s no-peer timeout with retry.

- [ ] **Step 1: Show a notice and return to menu on disconnect**

Modify `on_peer_disconnected` in `src/game/online.rs`:

```rust
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
```

- [ ] **Step 2: Add a connection timeout with retry**

Modify `js/online.js`'s `js_create_room` and `js_join_room` to start a 30-second timer that calls the error callback if `onPeerJoin` hasn't fired yet:

```js
const CONNECT_TIMEOUT_MS = 30000;

const withConnectTimeout = () => {
	const timer = setTimeout(() => {
		wasm && wasm.on_connection_error();
	}, CONNECT_TIMEOUT_MS);
	return () => clearTimeout(timer);
};

export const js_create_room = () => {
	const code = generateRoomCode();
	room = joinRoom({ appId: APP_ID, turnConfig: TURN_CONFIG }, code, {
		onJoinError: () => wasm && wasm.on_connection_error(),
	});
	const clearTimeoutFn = withConnectTimeout();
	room.onPeerJoin = () => {
		clearTimeoutFn();
		wasm && wasm.on_peer_connected();
	};
	room.onPeerLeave = () => wasm && wasm.on_peer_disconnected();

	initAction = room.makeAction('init');
	moveAction = room.makeAction('move');
	initAction.onMessage = data => {
		wasm && wasm.on_init_received(data.deck, data.hand1, data.hand2);
	};
	moveAction.onMessage = data => {
		wasm && wasm.on_action_received(data.clicks);
	};

	return code;
};

export const js_join_room = code => {
	// see the Task 3 version of this function for why this is normalized
	const normalizedCode = code.trim().toUpperCase();
	room = joinRoom({ appId: APP_ID, turnConfig: TURN_CONFIG }, normalizedCode, {
		onJoinError: () => wasm && wasm.on_connection_error(),
	});
	const clearTimeoutFn = withConnectTimeout();
	room.onPeerJoin = () => {
		clearTimeoutFn();
		wasm && wasm.on_peer_connected();
	};
	room.onPeerLeave = () => wasm && wasm.on_peer_disconnected();

	initAction = room.makeAction('init');
	moveAction = room.makeAction('move');
	initAction.onMessage = data => {
		wasm && wasm.on_init_received(data.deck, data.hand1, data.hand2);
	};
	moveAction.onMessage = data => {
		wasm && wasm.on_action_received(data.clicks);
	};
};
```

(This inlines `attachRoomListeners` into each function so the timeout can be cleared specifically on the first peer-join; remove the now-unused standalone `attachRoomListeners` function.)

- [ ] **Step 3: Show the timeout/error message and allow retry**

Modify `on_connection_error` in `src/game/online.rs`:

```rust
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
```

Since `create_room`/`join_room` in `src/menu.rs`'s listeners already re-run the whole flow on each button click, retrying is just clicking Create/Join again — no additional retry-button UI is needed.

- [ ] **Step 4: Add the i18n strings for the status messages**

Modify `js/i18n.js`. The status text is set directly from Rust (`set_text_content`) rather than via `data-i18n`, so it doesn't automatically switch with the language toggle. Given the existing `gameover-WINNER` text has this same limitation already (documented as a known scope limitation in an earlier session), apply the same approach here: leave these three Rust-set status strings in English for now, consistent with that existing precedent, and note it in the final summary.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check --lib`
Expected: no errors.

- [ ] **Step 6: Verify with two Playwright browser contexts**

1. Repeat Task 6 Step 7's create/join/connect flow.
2. Once both sides show the game screen, close context B entirely (`await contextB.close()`).
3. In context A, wait ~1s and screenshot: it should show the "opponent disconnected" status and be back at the Play Online panel (not the game canvas).
4. Separately, open a single context, click Create Game, and do **not** join from anywhere — this plan doesn't require waiting the full 30s to verify in CI, but do run it once manually to confirm the message appears (or reduce `CONNECT_TIMEOUT_MS` temporarily while testing, then restore it).

- [ ] **Step 7: Commit**

```bash
git add src/game/online.rs js/online.js js/i18n.js
git commit -m "Handle online match disconnects and connection timeouts"
```
