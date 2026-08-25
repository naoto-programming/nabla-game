# Online Multiplayer Design

## Summary

Add a "Play Online" mode: two friends, on different networks, connect
directly to each other (peer-to-peer, no game server) and play a live
match. One player creates a room and shares a link or short code; the
other joins with it.

## Decisions made with the user

- **Connection model**: 1-on-1 friend invite. No matchmaking/lobby.
- **Transport**: WebRTC peer-to-peer, with a free public TURN relay as
  an automatic fallback for connections that can't traverse NAT
  directly. Signaling (the handshake needed before a P2P channel
  exists) goes through existing public infrastructure via the
  [Trystero](https://github.com/dmotz/trystero) library (it supports
  several public backends, eg. BitTorrent trackers, Nostr relays) --
  no server or account of our own, ever.
- **Joining**: both a shareable link (`?room=CODE` auto-fills/joins)
  and a short manually-typed code are supported; they're the same
  underlying room ID either way.
- **Disconnect handling (v1)**: if either side drops, the match ends.
  No reconnection/resume. The remaining player sees a notice and
  returns to the menu.

## Architecture

```
┌─────────────────────┐        Trystero data channel        ┌─────────────────────┐
│   Player A (host)    │ ◄──────────────────────────────────► │  Player B (joiner)   │
│                       │                                      │                       │
│  src/game/online.rs   │                                      │  src/game/online.rs   │
│  (Rust/WASM)          │                                      │  (Rust/WASM)          │
│         ▲             │                                      │         ▲             │
│         │ wasm-bindgen │                                      │         │ wasm-bindgen │
│         ▼             │                                      │         ▼             │
│   js/online.js         │                                      │   js/online.js         │
│   (wraps Trystero)     │                                      │   (wraps Trystero)     │
└─────────────────────┘                                      └─────────────────────┘
```

**New files:**
- `js/online.js` -- wraps Trystero: `createRoom(roomId)` /
  `joinRoom(roomId)`, exposes `js_send_action(bytes)` and a callback
  registered from Rust for incoming messages/connect/disconnect
  events, mirroring the existing `js/katex.js` wasm-bindgen pattern.
- `src/game/online.rs` -- owns online-session state: room id, which
  player number (1 or 2) the local browser is, connection status, and
  the outgoing action buffer (below). Provides the functions
  `create_room()`, `join_room(code)`, and the message
  handlers wired from JS.
- `src/game/card_encoding.rs` -- maps every `Card` variant to a single
  `u8` and back. `Card` is a plain enum of plain enums (21 total
  variants across `BasisCard`/`AlgebraicCard`/`DerivativeCard`/
  `LimitCard`), so this is a small manual match, not a new dependency.
  No `serde`: the only things ever serialized are lists of cards.

**Changed files:**
- `src/game/structs.rs` -- new `GameState::PLAYONLINE` variant,
  alongside the existing `PLAYVS`/`PLAYAI` (neither is modified).
- `src/events/mousedown_handler.rs` -- the access-control guard added
  for the AI (block processing a click that isn't the acting player's
  own turn) extends to online mode: in `PLAYONLINE`, the local browser
  may only act on turns matching its assigned player number; the
  other player's turns are driven exclusively by replaying received
  network messages, the same way AI moves are replayed today via
  `branch_turn_phase`.
- `static/index.html`, `js/i18n.js`, `src/menu.rs` -- new menu button
  and Create/Join panel (see UI section).
- `package.json` -- add `trystero` as an npm dependency.

## Data flow

### 1. Starting a match

The deck shuffle is the only place true randomness enters a game
(`Field::new()` is fixed; hands are dealt from the shuffled deck).
Both sides must end up with the *identical* deck and hands, so only
one side generates them:

1. Host creates a room (`js/online.js` generates a room code,
   `trystero.joinRoom(appId, roomId)`), sees "Waiting for
   opponent...".
2. Joiner enters the code (typed, or auto-filled from `?room=`) and
   connects to the same room.
3. Trystero fires a peer-joined event on both sides.
4. **Host only**: runs `Game::new()` locally as normal (respecting the
   host's own Card Count settings from last session), then encodes
   `{ deck, player_1_hand, player_2_hand }` as three byte arrays via
   `card_encoding` and sends them as an `init` message. Host is always
   player 1.
5. **Joiner**: on receiving `init`, builds its local `Game` directly
   from the decoded arrays (its own RNG and Card Count settings are
   not used for this match). Joiner is always player 2.
6. Both sides switch to `GameState::PLAYONLINE` and the game screen
   appears at the same moment for both.

### 2. Relaying moves

Every real gameplay action already reduces to a sequence of
`RenderId` clicks fed through `branch_turn_phase` -- this is the exact
mechanism already built for the AI (`src/game/ai.rs`), reused here
unchanged for replay.

- While it's the **local** player's turn, `online.rs` appends every
  click's `RenderId` to an outgoing buffer, **except**
  `RenderId::Confirm` and `RenderId::Cancel`:
  - `Confirm` is excluded because it's a pure local commit of an
    already-fully-specified move; if the *peer's* own
    `CONFIRM_BEFORE_PLAY` setting is on, replaying the core clicks
    will land them in `TurnPhase::CONFIRM` too, and their side
    auto-confirms itself (same as the AI already does). This lets
    each player keep an independent local preference for that
    setting without the two sides' click sequences needing to match.
  - `Cancel` is excluded (and clears the buffer) because it means
    nothing was actually committed -- there's nothing to replay.
- When `next_turn()` fires (the turn actually completed), the buffered
  sequence is sent as one `action` message, then the buffer is
  cleared.
- On receiving an `action` message, `online.rs` replays each
  `RenderId` in order via `branch_turn_phase(id, remote_player_num)`,
  then auto-confirms if that leaves the local side awaiting
  confirmation (identical to the AI's own post-replay check).

This means the network protocol is just two message types:
`init { deck, hand_1, hand_2 }` (once) and `action { clicks: [RenderId] }`
(once per completed turn) -- no full state resyncing, no server-side
validation, matching the same "both sides run the same deterministic
WASM and trust the replay" model already used for the AI opponent.

### 3. Ending

- Either player winning (existing `game_over` flow) works unchanged --
  it's purely local to each side once both have replayed the same
  moves.
- Peer disconnects (Trystero's peer-leave event) at any point: show
  "Your opponent disconnected" and return to the menu. No resume.

## UI

- Main menu: new "Play Online" button, alongside "Play vs Friend" /
  "Play vs AI".
- Its panel offers "Create Game" / "Join Game".
  - **Create**: generates a 6-character code (uppercase, excluding
    ambiguous characters like `0`/`O`, `1`/`I`/`L`), shows it plus a
    `?room=CODE` link with a copy button, and a "Waiting for
    opponent..." status.
  - **Join**: a code input (auto-filled if the page was opened via a
    `?room=` link) and a "Connect" button.
- On page load, a `?room=` query param pre-fills and can auto-navigate
  straight to the Join panel for a "click the link, land ready to
  join" experience.
- Connection status text covers: waiting, connecting, connected,
  opponent disconnected, and a connection-timeout error (with retry).

## Error handling

- **No peer joins in time**: a timeout (30s) surfaces "Couldn't
  connect" with a retry option, rather than waiting forever.
- **Peer disconnects mid-game**: notice + return to menu (decided
  above).
- **Unexpected/out-of-turn message**: logged and ignored rather than
  applied or panicking -- this is a casual game, not an
  anti-cheat-hardened one, so defensive-but-quiet is enough.

## Testing

- `card_encoding`'s round-trip (`Card` -> `u8` -> `Card` for every
  variant) is a plain unit test, no browser needed.
- The connection/replay flow needs two independent peers to exercise
  for real. Verified the way this session has verified every other
  feature: two separate Playwright browser contexts against the
  deployed Pages URL, one creating a room and the other joining via
  the generated code, playing several moves each way and checking
  both sides render the same field/hand state.

## Explicitly out of scope for v1

- Matchmaking / public lobbies.
- Reconnection after a drop.
- Spectators.
- Any server-authoritative validation (both clients are trusted).
