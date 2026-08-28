# Self-Play Training Viewer Design

## Summary

Add a spectator view for the background self-play reinforcement-learning
system (`src/game/learning.rs`, added in the "Replace minimax with a
persistent self-play learning system" commit). While "AI Learning Mode" is
on, the Settings panel gets a live board that looks exactly like a real
match (same canvas rendering code, same card art), showing one of several
self-play games in progress, with buttons to switch which game is shown.
The bulk (unwatched) training loop that currently drives most of the
learned-table growth is untouched; a separate, smaller pool of games is
added purely so there's something steppable/watchable to switch between.

## Decisions made with the user

- **No slow-motion pacing needed**: the viewer does not need to show moves
  at a human-readable pace -- watching the board update rapidly ("blazing
  fast") is the goal, not a narrated step-by-step. This means the watched
  pool can run at whatever speed the tick loop naturally produces, with no
  artificial throttling.
- **Two-tier execution**: the existing bulk background loop
  (`run_self_play_batch`, 3 games/150ms tick, run-to-completion, unwatched)
  stays as the primary source of learned patterns. A new, separate pool of
  **10** games is added that step one move at a time (watchable,
  switchable), crediting the same shared learned table when each finishes.
  WASM is single-threaded, so "10 concurrent games" means 10 independently-
  stepped game states interleaved on the same tick, not literal parallel
  execution.
- **Throughput tuning**: the user wants to revisit `SELF_PLAY_BATCH_SIZE`
  (currently 3) and `SELF_PLAY_TICK_MS` (currently 150) as part of this
  work, to raise bulk-tier throughput. Because this is a CPU-load/jank
  trade-off best judged against the user's own browser (increasing batch
  size makes each tick heavier -> more stutter risk during a real match in
  progress; shortening tick interval keeps each tick's cost the same but
  raises total CPU time spent per second), rather than hardcoding new
  constants, both become **live-adjustable number inputs** in Settings
  (next to the existing checkbox), pre-filled with today's values (3 /
  150) as the starting point. Changing either restarts the background
  `Interval` with the new value if learning mode is currently on. This
  gives the user direct, no-code-change control to find their own
  comfortable throughput/jank trade-off, rather than the plan guessing a
  number on their behalf.
- **UI placement**: extend the existing Settings > "AI Learning Mode"
  section (checkbox + progress readout, `static/index.html`) rather than
  adding a new top-level menu entry. Settings is only reachable from the
  main menu (`GameState::MENU`), never as a pause overlay during a live
  match (confirmed via `menu.rs`'s top-level button list and
  `Game::set_state`'s `_ => {}` fallthrough for non-PLAYAI/PLAYVS states),
  so there is no real in-progress match's `GAME` state at risk when the
  viewer is open.

## Architecture

```
┌───────────────────────────────────────────────────────────┐
│ self_play_tick (existing single Interval, 150ms)            │
│                                                               │
│  1. run_self_play_batch(BATCH_SIZE)   -- bulk, unwatched     │
│  2. step every TrainingSlot in TRAINING_POOL by one move     │
│     (finished slot -> record_game_outcome -> fresh slot)     │
│  3. if the viewer panel is open: mirror the selected slot's  │
│     field/hands into GAME, call render::draw()               │
└───────────────────────────────────────────────────────────┘
```

**New in `src/game/learning.rs`:**

- `struct TrainingSlot { field, player_1, player_2, deck, turn_number,
  difficulty_1, difficulty_2, recorded_moves }` -- the loop-local state
  `simulate_self_play_game` currently holds inline, pulled out so a single
  game can be advanced one move at a time across ticks instead of run to
  completion in one call.
- `impl TrainingSlot { fn new(d1, d2) -> Self; fn step(&mut self) ->
  SlotStep }`, where `SlotStep` is `Continue | Finished(u32) |
  Stalled` (hit `SELF_PLAY_TURN_LIMIT`). `step` plays exactly one move
  (or forfeits a stuck turn, mirroring the existing empty-candidates
  handling), reusing `generate_candidates_for`/`choose_move`/
  `learning_key` exactly as today.
- `simulate_self_play_game` becomes a thin wrapper: `TrainingSlot::new` +
  loop calling `step` until `Finished`/`Stalled`. Its public signature and
  the 8 existing tests in `learning.rs` are unaffected.
- `static mut TRAINING_POOL: Vec<TrainingSlot>` -- fixed-size pool,
  `POOL_SIZE: usize = 10`, lazily initialized (same `Option`-wrapped-static
  pattern as `LEARNED_TABLE`).
- `static mut SELECTED_TRAINING_SLOT: usize = 0`.
- `fn step_training_pool()` -- called from `self_play_tick`; steps every
  slot once, credits + replaces any that finished.
- `fn select_training_slot(index: usize)` -- called from the new UI
  buttons.
- `SELF_PLAY_BATCH_SIZE` and `SELF_PLAY_TICK_MS` change from `const` to
  `static mut` (still 3 / 150 as their initial values), plus
  `fn set_self_play_batch_size(n: u32)` and `fn set_self_play_tick_ms(ms:
  u32)` -- the latter restarts the `Interval` (via
  `stop_self_play_loop`/`start_self_play_loop`) with the new duration if
  learning mode is currently enabled, so a change takes effect
  immediately rather than after a toggle off/on.
- `fn sync_training_viewer_display()` -- called from `self_play_tick`
  every tick, regardless of visibility (cheap when hidden -- see below).
  Visibility is read from a DOM check (does `#training-viewer-panel` have
  the `hidden` attribute?), mirroring how `update_progress_display`
  already best-effort-queries the DOM and no-ops if the element isn't
  there.
  - **Required correctness guard**: `GAME::new()` is only ever called at
    page load, when hosting an online match, and from the "Restart?"
    GAMEOVER button (`grep`-verified -- no other call site). Starting a
    new PLAYAI/PLAYVS match from the main menu does *not* reset `GAME`;
    it reuses whatever `field`/`player_1`/`player_2` are already sitting
    in the singleton. If the viewer overwrites those fields and nothing
    ever puts them back, leaving the viewer and starting/resuming a real
    match would start from stale self-play data instead of the player's
    actual board.
  - Fix: a `static mut GAME_SNAPSHOT_BEFORE_VIEWING: Option<(Field,
    Vec<Card>, Vec<Card>, Vec<Card>)>` (field, player_1, player_2, deck).
    While the panel is visible: snapshot `GAME`'s current
    field/hands/deck into it (only if `None`, so repeated ticks don't
    overwrite the snapshot with already-mirrored training data), then
    copy the selected `TrainingSlot`'s field/hands/deck into `GAME` and
    call `render::draw()`. While the panel is hidden: if a snapshot is
    present, restore it into `GAME` (`Option::take`, so this fires
    exactly once per viewing session) and redraw; otherwise no-op. Because
    this runs every tick, it self-heals regardless of *how* the panel
    became hidden -- "Back" button, the top-level "Main Menu" button, or
    navigating to another Settings sub-panel -- with no changes needed to
    those existing button handlers.
  - One more exit path the tick can't cover: turning "AI Learning Mode"
    off *while the viewer panel is still open* stops the tick entirely,
    so the self-healing above would never run. `set_learning_mode_enabled(false)`
    must restore `GAME_SNAPSHOT_BEFORE_VIEWING` unconditionally (if
    present) right after `stop_self_play_loop()`, independent of the DOM
    visibility check.

**Changed files:**

- `src/events/mousedown_handler.rs` -- **required correctness fix**:
  `handle_mousedown` currently has no top-level `GameState` guard (the
  existing PLAYAI/PLAYONLINE checks only special-case *within* those
  states). Once the viewer can mirror live field/hand data into `GAME`
  while `GameState` is `MENU`/`SETTINGS`, a click on the visible canvas
  would fall through into the same click-handling logic a real match uses,
  mutating `GAME` out of sync with the `TrainingSlot` it was mirrored from.
  Add an early return at the top of `handle_mousedown`:
  `if !matches!(game.state, GameState::PLAYAI | GameState::PLAYVS |
  GameState::PLAYONLINE) { return; }` -- making the viewer's board
  read-only, and incidentally closing a latent gap in the existing code
  (nothing today prevents this same class of stray click if a field/hand
  ever gets rendered outside a live match state).
- `static/index.html` -- wraps `menu-SETTINGS`'s existing content (AI
  Difficulty select through the "Change Card Counts" button) in a new
  `#settings-main-content` container so it can be hidden as a block while
  the viewer is open. Inside the existing "AI Learning Mode" section: a
  "Watch" button, plus a new `#training-viewer-panel` sub-panel (hidden by
  default, following the `online-create-panel`/`online-join-panel`
  pattern already used for `menu-PLAYONLINE` -- a plain nested `<div>`,
  not a `Menu::activate`-level `.menu-item`) containing 10 slot-select
  buttons ("Game 1".."Game 10") and a "Back" button. No new `<canvas>` --
  see Viewer panel layout below. Two number inputs, "Batch size per tick"
  and "Tick interval (ms)" (pre-filled 3 / 150), are added alongside the
  existing checkbox, inside `#settings-main-content` (not the viewer
  sub-panel).
- `static/index.css` -- `#menu.viewing { background: none; }`, plus a
  small backing style for `#training-viewer-panel`'s controls (opaque bar
  pinned to the top of the viewport) so they stay legible over the
  animated board.
- `js/i18n.js` -- labels for the new controls.
- `src/menu.rs` -- wiring for the slot-select buttons and throughput
  preset control, following the existing `AI_LEARNING_MODE` checkbox
  wiring pattern (`"AI_LEARNING_MODE" => set_learning_mode_enabled(...)`).

## Rendering approach

The viewer reuses `render::draw()` and the existing `GAME`/`CANVAS`
singletons unchanged -- no parallel rendering path, no second canvas. This
is what makes the board "look exactly like a real match" (same card art,
same KaTeX-rendered expressions, same layout code) rather than a
reimplementation. The trade-off is the `mousedown_handler.rs` guard above,
which is required for this reuse to be safe.

## Data flow

1. `set_learning_mode_enabled(true)` (existing) starts `self_play_tick` on
   its existing `Interval` as today, and now also lazily initializes
   `TRAINING_POOL` with 10 fresh `TrainingSlot`s.
2. Every tick: bulk batch runs (unchanged), then every pool slot steps by
   one move. A slot that just finished has its `recorded_moves` folded
   into the learned table via the existing `record_game_outcome`, then is
   immediately replaced by `TrainingSlot::new(..)` -- no idle slots.
3. Every tick, `sync_training_viewer_display()` checks
   `#training-viewer-panel`'s visibility: if visible, snapshots `GAME`'s
   real field/hands/deck on first entry, then mirrors
   `TRAINING_POOL[SELECTED_TRAINING_SLOT]` into `GAME` and redraws; if
   hidden and a snapshot exists, restores it into `GAME` once and redraws.
4. Clicking a "Game N" button calls `select_training_slot(N)`; the next
   tick's mirror step picks it up. No slot is paused or restarted by
   switching -- all 10 keep advancing regardless of which is displayed.
5. Turning learning mode off stops the tick (existing behavior), restores
   `GAME_SNAPSHOT_BEFORE_VIEWING` if one is pending (see above), and
   leaves `TRAINING_POOL` as-is (frozen) until re-enabled, which
   reinitializes it fresh.

## Testing

- `TrainingSlot::step` / `SlotStep` variants: unit tests analogous to the
  existing `test_self_play_game_terminates_with_a_winner_or_no_result`,
  but driving `step` in a loop from a test and asserting the same
  Finished/Stalled outcomes, plus a test that stepping to completion via
  `TrainingSlot` produces an identical-shaped result to the current
  `simulate_self_play_game` (same win condition, same recorded-move
  count invariants).
- `step_training_pool`: a test that after N ticks, every slot has
  advanced (turn_number increased or been replaced by a fresh slot with
  turn_number 0), and the learned table's pattern count is non-decreasing.
- The mirrored render path and the `mousedown_handler.rs` guard need
  real-browser verification (per this project's existing pattern of
  Playwright-driven checks against the dev build): open Settings, enable
  AI Learning Mode, click "Watch", confirm the board animates, switch
  between a few "Game N" buttons and confirm the displayed board changes,
  click directly on the displayed board and confirm nothing happens (no
  SELECT/CONFIRM phase entered, `GAME.state` unchanged), then click "Back"
  and confirm the mirror-and-redraw step stops (no further canvas
  updates from the pool). Also verify the snapshot/restore guard: note
  the real field/hand state (or lack thereof) before clicking "Watch",
  watch training games for a few seconds, click "Back" (and separately,
  in another pass, exit via the top "Main Menu" button instead of "Back"),
  then start a "Play vs AI" match and confirm it begins from a fresh
  board, not from leftover self-play data.

## Viewer panel layout

`static/index.html` has `#menu` (all menu panels, including
`menu-SETTINGS`) as a sibling of `#canvas`, not nested inside it, the same
way `menu-PLAYONLINE` already nests togglable sub-panels
(`online-create-panel` / `online-join-panel`) inside itself for its
Create/Join flow. `#menu` is `position: fixed`, full-viewport, `z-index:
3`, with an **opaque** `background-color: white` + `bg.png`
(`static/index.css:71-78`) -- it fully covers both `#canvas` (`z-index:
1`) and `#katex` (`z-index: 2`, the absolutely-positioned DOM layer KaTeX
renders card expressions into, matching canvas pixel coordinates -- the
board's math text is *not* part of the canvas bitmap). Reusing
`render::draw()` alone is therefore not enough to make anything visible;
`#menu`'s own background has to become transparent too while the viewer
is open, or neither the canvas nor the card text underneath shows through.

The viewer panel: a new "Watch" button inside the AI Learning Mode section
swaps the panel's content to a `#training-viewer-panel` sub-panel (hidden
by default, like the online sub-panels), and toggles a `viewing` CSS class
on `#menu` itself (`element.class_list().add_1("viewing")` /
`.remove_1("viewing")` from `menu.rs`, alongside the existing
hidden-attribute toggling). A new CSS rule,
`#menu.viewing { background: none; }`, is the only new styling needed to
reveal `#canvas`/`#katex` underneath at their normal full position and
coordinates -- no second `<canvas>`, no `Canvas`/`CANVAS` singleton
changes. `#training-viewer-panel`'s own controls (the 10 slot-select
buttons, a "Back" button) need their own small opaque backing box (plain
CSS, e.g. a semi-opaque bar pinned to the top of the viewport) so they
stay legible against the moving board underneath. "Back" removes the
`viewing` class, hides `#training-viewer-panel`, and returns to the
normal Settings list; the mirror-and-redraw step stops on the next tick
once `#training-viewer-panel` is hidden again (per the visibility check in
`mirror_selected_slot_into_game`).

## Explicitly out of scope

- Pausing/rewinding/stepping a specific watched game by hand (move-by-move
  scrubbing) -- the user only wants a fast, always-live view.
- Persisting which slot was selected across page reloads.
- Configurable pool size beyond the fixed 10 (can be revisited later if
  10 turns out to be the wrong number in practice).
