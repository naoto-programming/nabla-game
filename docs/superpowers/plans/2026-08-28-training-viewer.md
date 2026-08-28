# Self-Play Training Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a player watch a background self-play game in progress, using the exact same board rendering as a real match, and switch between 10 concurrently-running training games without slowing down training.

**Architecture:** Refactor the existing run-to-completion self-play simulation into a steppable `TrainingSlot`, keep a pool of 10 of them advancing one move per tick alongside the existing (unwatched) bulk training loop, and reuse the live `GAME` singleton + `render::draw()` to display whichever slot is selected -- snapshotting and restoring `GAME`'s real state around the viewing session so a real match is never corrupted by training data.

**Tech Stack:** Rust/WASM (wasm-bindgen), gloo (Interval, EventListener), plain DOM/CSS via `web-sys`, no new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-28-training-viewer-design.md`

## Global Constraints

- Pool size is fixed at 10 (`POOL_SIZE`), not user-configurable in this version.
- The viewer must never let a real click mutate `GAME` -- `handle_mousedown` must guard on `GameState`.
- `GAME`'s real field/hands/deck must always be restored after viewing, regardless of exit path (Back button, Main Menu button, or disabling AI Learning Mode while the viewer is open).
- No second `<canvas>` -- the viewer reuses the existing `#canvas`/`#katex` elements and the existing `Canvas`/`GAME` singletons.
- Batch size and tick interval become live-adjustable UI inputs (default 3 / 150, matching today's values), not new hardcoded constants.
- All plain-Rust logic (no DOM) must be covered by `cargo test`; anything touching `GAME`/DOM is verified manually in the dev build (this project has no existing DOM-level automated test coverage to extend -- see spec's Testing section).

---

## Task 1: Extract `TrainingSlot` (steppable self-play state)

**Files:**
- Modify: `src/game/learning.rs:372-434` (replaces the body of `simulate_self_play_game`)
- Test: `src/game/learning.rs` (`#[cfg(test)] mod tests`, same file)

**Interfaces:**
- Consumes: existing `generate_candidates_for`, `choose_move`, `learning_key`, `side_is_cleared`, `opponent_of`, `get_new_deck`, `create_players`, `SELF_PLAY_TURN_LIMIT`, `AiDifficulty` (all already imported in this file).
- Produces: `enum SlotStep { Continue, Finished(u32), Stalled }` and `struct TrainingSlot { field: Field, player_1: Vec<Card>, player_2: Vec<Card>, deck: Vec<Card>, turn_number: u32, difficulty_1: AiDifficulty, difficulty_2: AiDifficulty, recorded_moves: Vec<(u32, u64)> }` with `TrainingSlot::new(difficulty_1: AiDifficulty, difficulty_2: AiDifficulty) -> Self` and `TrainingSlot::step(&mut self) -> SlotStep`. Task 2 constructs pools of these and calls `.step()`; Task 4 reads `.field`/`.player_1`/`.player_2`/`.deck` for display.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block at the bottom of `src/game/learning.rs` (after the existing `test_self_play_batch_grows_the_learned_table` test):

```rust
    #[test]
    fn test_training_slot_step_reaches_finished_or_stalled() {
        for _ in 0..20 {
            let mut slot = TrainingSlot::new(AiDifficulty::Medium, AiDifficulty::Medium);
            loop {
                match slot.step() {
                    SlotStep::Continue => continue,
                    SlotStep::Finished(winner) => {
                        assert!(winner == 1 || winner == 2);
                        assert!(!slot.recorded_moves.is_empty());
                        break;
                    }
                    SlotStep::Stalled => break,
                }
            }
        }
    }

    #[test]
    fn test_training_slot_new_starts_at_turn_zero_with_full_hands() {
        let slot = TrainingSlot::new(AiDifficulty::Easy, AiDifficulty::Hard);
        assert_eq!(slot.turn_number, 0);
        assert_eq!(slot.player_1.len(), 7);
        assert_eq!(slot.player_2.len(), 7);
        assert!(slot.recorded_moves.is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test training_slot -- --nocapture`
Expected: FAIL to compile (`TrainingSlot`/`SlotStep` not found in this scope).

- [ ] **Step 3: Implement `SlotStep` and `TrainingSlot`**

In `src/game/learning.rs`, replace the entire `simulate_self_play_game` function (lines 358-434, including its doc comment) with:

```rust
/// outcome of advancing a TrainingSlot by exactly one move
#[derive(Debug, PartialEq)]
enum SlotStep {
    /// the game is still going
    Continue,
    /// the game just ended; carries the winning player number
    Finished(u32),
    /// hit SELF_PLAY_TURN_LIMIT without a winner -- caller should discard this
    /// game's moves rather than credit a fabricated result
    Stalled,
}

/// one self-play game's state, advanced one move at a time via `step` -- unlike
/// the old run-to-completion simulation this replaces, a TrainingSlot can be
/// paused across ticks, which is what lets the training viewer show a game
/// still in progress instead of only ever seeing a finished result
struct TrainingSlot {
    field: Field,
    player_1: Vec<Card>,
    player_2: Vec<Card>,
    deck: Vec<Card>,
    turn_number: u32,
    difficulty_1: AiDifficulty,
    difficulty_2: AiDifficulty,
    recorded_moves: Vec<(u32, u64)>,
}

impl TrainingSlot {
    /// deals a fresh shuffled game, same setup simulate_self_play_game used to
    /// do inline
    fn new(difficulty_1: AiDifficulty, difficulty_2: AiDifficulty) -> Self {
        let mut deck = get_new_deck();
        deck.shuffle(&mut rand::thread_rng());
        let (player_1, player_2) = create_players(&mut deck);
        TrainingSlot {
            field: Field::new(),
            player_1,
            player_2,
            deck,
            turn_number: 0,
            difficulty_1,
            difficulty_2,
            recorded_moves: vec![],
        }
    }

    /// plays exactly one move (or forfeits a stuck turn, same as the real AI's
    /// own empty-candidates handling) -- the per-turn body of the old
    /// simulate_self_play_game loop, unchanged in behaviour
    fn step(&mut self) -> SlotStep {
        if self.turn_number > SELF_PLAY_TURN_LIMIT {
            return SlotStep::Stalled;
        }

        let mover = if self.turn_number % 2 == 0 { 1 } else { 2 };
        let difficulty = if mover == 1 { self.difficulty_1 } else { self.difficulty_2 };
        let opponent_hand = if mover == 1 { self.player_2.clone() } else { self.player_1.clone() };
        let mover_hand = if mover == 1 { &mut self.player_1 } else { &mut self.player_2 };

        let candidates = generate_candidates_for(mover, mover_hand, &self.field, self.turn_number);
        if candidates.is_empty() {
            self.turn_number += 1;
            return SlotStep::Continue;
        }

        let chosen = choose_move(
            candidates,
            mover,
            mover_hand,
            &opponent_hand,
            difficulty,
            self.turn_number,
            &self.field,
        );
        let key = learning_key(&self.field, &chosen.resulting_field, mover, self.turn_number, mover_hand);
        self.recorded_moves.push((mover, key));

        let mut consumed = chosen.consumed_hand_indices.clone();
        consumed.sort_unstable();
        consumed.reverse();
        for idx in consumed {
            mover_hand.remove(idx);
        }
        self.field = chosen.resulting_field;

        if side_is_cleared(&self.field, opponent_of(mover)) {
            return SlotStep::Finished(mover);
        }

        let cards_to_deal = 7usize.saturating_sub(mover_hand.len()).min(self.deck.len());
        for _ in 0..cards_to_deal {
            if let Some(card) = self.deck.pop() {
                mover_hand.push(card);
            }
        }

        self.turn_number += 1;
        SlotStep::Continue
    }
}

/// plays one full game entirely in memory by stepping a TrainingSlot to
/// completion in a single call -- kept as a thin wrapper (rather than removed)
/// since run_self_play_batch and the existing tests below still use this
/// run-to-completion shape for the unwatched bulk training loop
pub fn simulate_self_play_game(difficulty_1: AiDifficulty, difficulty_2: AiDifficulty) -> Option<(u32, Vec<(u32, u64)>)> {
    let mut slot = TrainingSlot::new(difficulty_1, difficulty_2);
    loop {
        match slot.step() {
            SlotStep::Continue => continue,
            SlotStep::Finished(winner) => return Some((winner, slot.recorded_moves)),
            SlotStep::Stalled => return None,
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS -- all 8 pre-existing tests in `learning.rs` plus the 2 new ones (10 total), unaffected in behaviour since `simulate_self_play_game`'s external signature and semantics are unchanged.

- [ ] **Step 5: Commit**

```bash
git add src/game/learning.rs
git commit -m "$(cat <<'EOF'
Extract a steppable TrainingSlot from simulate_self_play_game

Pulls the run-to-completion self-play loop's state into a struct that
can be advanced one move at a time via step(), so a game's progress
can be paused across ticks. simulate_self_play_game becomes a thin
wrapper that steps a TrainingSlot to completion, unchanged in
behaviour -- this is prep for the training viewer, which needs
watchable games that don't finish in a single call.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: 10-slot watchable training pool

**Files:**
- Modify: `src/game/learning.rs` (add pool state near `run_self_play_batch`; wire into `self_play_tick`)
- Test: same file's `mod tests`

**Interfaces:**
- Consumes: `TrainingSlot`, `SlotStep`, `AiDifficulty`, `record_game_outcome` (all from Task 1 / existing code).
- Produces: `const POOL_SIZE: usize = 10`, `fn step_training_pool()`, `pub fn select_training_slot(index: usize)`, and the statics `TRAINING_POOL: Option<Vec<TrainingSlot>>` / `SELECTED_TRAINING_SLOT: usize`. Task 4 reads `training_pool()[SELECTED_TRAINING_SLOT]` for display; `menu.rs` (Task 8) calls `select_training_slot`.

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
    #[test]
    fn test_step_training_pool_grows_the_learned_table() {
        load_table(HashMap::new());
        unsafe {
            TRAINING_POOL = None;
        }
        // more than SELF_PLAY_TURN_LIMIT so every one of the 10 slots completes
        // (or stalls out) at least one full game
        for _ in 0..(SELF_PLAY_TURN_LIMIT + 50) {
            step_training_pool();
        }
        assert!(
            learned_pattern_count() > 0,
            "stepping a 10-slot pool through at least one full game each should teach at least one pattern"
        );
    }

    #[test]
    fn test_select_training_slot_updates_selection_within_bounds() {
        unsafe {
            SELECTED_TRAINING_SLOT = 0;
        }
        select_training_slot(3);
        assert_eq!(unsafe { SELECTED_TRAINING_SLOT }, 3);
        select_training_slot(POOL_SIZE); // out of bounds -- ignored
        assert_eq!(unsafe { SELECTED_TRAINING_SLOT }, 3);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test training_pool -- --nocapture` and `cargo test select_training_slot -- --nocapture`
Expected: FAIL to compile (`step_training_pool`/`select_training_slot`/`TRAINING_POOL`/`SELECTED_TRAINING_SLOT`/`POOL_SIZE` not found).

- [ ] **Step 3: Implement the pool**

In `src/game/learning.rs`, immediately after `run_self_play_batch` (the function added at the end of the "Headless self-play" section from Task 1), add:

```rust
/* ---------------------------------------------------------------------------
 * Watchable training pool -- a small set of self-play games stepped one move
 * per tick (unlike run_self_play_batch's run-to-completion games above), so
 * the training viewer always has something in-progress to display. Runs
 * alongside the bulk loop, not instead of it: this pool is a tiny fraction of
 * total training throughput, purely there to be watchable.
 * ------------------------------------------------------------------------- */

const POOL_SIZE: usize = 10;

static mut TRAINING_POOL: Option<Vec<TrainingSlot>> = None;
static mut SELECTED_TRAINING_SLOT: usize = 0;

const POOL_DIFFICULTIES: [AiDifficulty; 3] = [AiDifficulty::Easy, AiDifficulty::Medium, AiDifficulty::Hard];

fn fresh_training_slot(i: usize) -> TrainingSlot {
    let difficulty_1 = POOL_DIFFICULTIES[i % POOL_DIFFICULTIES.len()];
    let difficulty_2 = POOL_DIFFICULTIES[(i / POOL_DIFFICULTIES.len()) % POOL_DIFFICULTIES.len()];
    TrainingSlot::new(difficulty_1, difficulty_2)
}

fn training_pool() -> &'static mut Vec<TrainingSlot> {
    unsafe {
        if TRAINING_POOL.is_none() {
            TRAINING_POOL = Some((0..POOL_SIZE).map(fresh_training_slot).collect());
        }
        TRAINING_POOL.as_mut().unwrap()
    }
}

/// advances every pool slot by exactly one move. A slot that just finished (or
/// stalled) has any decisive result folded into the same learned table
/// run_self_play_batch feeds, then is replaced immediately so no slot ever
/// sits idle waiting to be picked again
fn step_training_pool() {
    let pool = training_pool();
    for i in 0..pool.len() {
        match pool[i].step() {
            SlotStep::Continue => {}
            SlotStep::Finished(winner) => {
                record_game_outcome(&pool[i].recorded_moves, winner);
                pool[i] = fresh_training_slot(i);
            }
            SlotStep::Stalled => {
                pool[i] = fresh_training_slot(i);
            }
        }
    }
}

/// switches which pool slot the training viewer displays -- called from the
/// "Game N" buttons in Settings (see menu.rs). Every slot keeps advancing
/// regardless of which one is selected; this only changes what gets mirrored
/// into GAME on the next tick (see sync_training_viewer_display)
pub fn select_training_slot(index: usize) {
    if index < POOL_SIZE {
        unsafe {
            SELECTED_TRAINING_SLOT = index;
        }
        // the newly-selected slot's board has nothing to do with whatever was
        // expanded on the previously-viewed one at the same screen position
        if let Some(canvas) = unsafe { crate::CANVAS.as_mut() } {
            canvas.expanded_cards.clear();
        }
    }
}
```

Then, in `self_play_tick`, add a call to `step_training_pool()` right after the existing `run_self_play_batch` call:

```rust
fn self_play_tick() {
    let decisive = run_self_play_batch(SELF_PLAY_BATCH_SIZE);
    step_training_pool();
    unsafe {
        GAMES_PLAYED_THIS_SESSION += decisive;
        TICKS_SINCE_SAVE += 1;
        if TICKS_SINCE_SAVE >= SAVE_EVERY_N_TICKS {
            TICKS_SINCE_SAVE = 0;
            save_persisted_table();
        }
    }
    update_progress_display();
}
```

(`SELF_PLAY_BATCH_SIZE` here is still the existing `const` from before Task 3 -- Task 3 changes how it's read, not this call site's shape.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS -- all previous tests plus the 2 new ones (12 total).

- [ ] **Step 5: Commit**

```bash
git add src/game/learning.rs
git commit -m "$(cat <<'EOF'
Add a 10-slot watchable training pool alongside the bulk loop

Every self_play_tick now also steps 10 independent TrainingSlots by
one move each -- unlike the bulk run-to-completion games, these stay
paused-in-progress across ticks so there's always something to look
at. A finished or stalled slot is credited (if decisive) and replaced
immediately. select_training_slot lets the (not yet built) viewer UI
choose which of the 10 to display.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Live-adjustable batch size and tick interval

**Files:**
- Modify: `src/game/learning.rs` (the "Background self-play loop" section: `SELF_PLAY_BATCH_SIZE`, `SELF_PLAY_TICK_MS`, `start_self_play_loop`, `self_play_tick`)
- Test: same file's `mod tests`

**Interfaces:**
- Produces: `pub fn set_self_play_batch_size(n: u32)`, `pub fn set_self_play_tick_ms(ms: u32)`. Task 8 wires these to two new number inputs in Settings.

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
    #[test]
    fn test_set_self_play_batch_size_and_tick_ms_apply_and_floor_at_minimum() {
        // learning mode must be off here (the default) -- turning it on would
        // start a real gloo Interval, which needs a browser window this test
        // doesn't have
        set_self_play_batch_size(9);
        assert_eq!(unsafe { SELF_PLAY_BATCH_SIZE }, 9);
        set_self_play_batch_size(0);
        assert_eq!(unsafe { SELF_PLAY_BATCH_SIZE }, 1, "a batch size of 0 would make the tick a permanent no-op");

        set_self_play_tick_ms(500);
        assert_eq!(unsafe { SELF_PLAY_TICK_MS }, 500);
        set_self_play_tick_ms(0);
        assert_eq!(unsafe { SELF_PLAY_TICK_MS }, 10, "a 0ms interval would busy-loop the tab");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_set_self_play_batch_size_and_tick_ms -- --nocapture`
Expected: FAIL to compile (`set_self_play_batch_size`/`set_self_play_tick_ms` not found; `SELF_PLAY_BATCH_SIZE`/`SELF_PLAY_TICK_MS` are still `const`, not assignable `static mut`).

- [ ] **Step 3: Make the constants runtime-adjustable**

In `src/game/learning.rs`, change:

```rust
const SELF_PLAY_BATCH_SIZE: u32 = 3;
const SELF_PLAY_TICK_MS: u32 = 150;
```

to:

```rust
static mut SELF_PLAY_BATCH_SIZE: u32 = 3;
static mut SELF_PLAY_TICK_MS: u32 = 150;
```

Both read sites need an `unsafe` block now that these are `static mut` instead of `const` -- but check what's already around each call before adding one, since a redundant nested `unsafe` triggers an `unused_unsafe` compiler warning.

`start_self_play_loop`'s body is already entirely wrapped in `unsafe { ... }`, so `SELF_PLAY_TICK_MS` there needs no new block -- leave `let interval = Interval::new(SELF_PLAY_TICK_MS, self_play_tick);` exactly as it is; it now compiles as a `static mut` read using the `unsafe` block that already surrounds it.

`self_play_tick`'s first line is *not* inside an unsafe block (only the statements below it are), so this one does need a new one. Change `let decisive = run_self_play_batch(SELF_PLAY_BATCH_SIZE);` to:

```rust
    let decisive = run_self_play_batch(unsafe { SELF_PLAY_BATCH_SIZE });
```

Then add the two setters, right after `stop_self_play_loop`:

```rust
/// updates the bulk loop's per-tick batch size -- takes effect on the very
/// next tick, no restart needed. Floors at 1 so a cleared/zeroed UI input
/// can't turn the tick into a permanent no-op
pub fn set_self_play_batch_size(n: u32) {
    unsafe {
        SELF_PLAY_BATCH_SIZE = n.max(1);
    }
}

/// updates the bulk loop's tick interval. Unlike batch size this needs the
/// running Interval rebuilt (gloo's Interval is fixed-duration once
/// constructed), so this restarts the loop if it's currently running. Floors
/// at 10ms so a cleared/zeroed UI input can't busy-loop the tab
pub fn set_self_play_tick_ms(ms: u32) {
    unsafe {
        SELF_PLAY_TICK_MS = ms.max(10);
    }
    if unsafe { LEARNING_MODE_ENABLED } {
        stop_self_play_loop();
        start_self_play_loop();
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS -- all previous tests plus the new one (13 total).

- [ ] **Step 5: Commit**

```bash
git add src/game/learning.rs
git commit -m "$(cat <<'EOF'
Make self-play batch size and tick interval live-adjustable

SELF_PLAY_BATCH_SIZE/SELF_PLAY_TICK_MS become static mut instead of
const, with set_self_play_batch_size/set_self_play_tick_ms setters
that take effect immediately (the tick interval restart included) --
throughput vs. jank is a trade-off best tuned against the user's own
browser, so this exposes it as a runtime control instead of a
hardcoded guess.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Mirror the selected slot into `GAME` (with snapshot/restore)

**Files:**
- Modify: `src/game/learning.rs` (imports, new functions, `self_play_tick`, `set_learning_mode_enabled`)

**Interfaces:**
- Consumes: `training_pool()`, `SELECTED_TRAINING_SLOT` (Task 2); `crate::GAME`, `crate::render::render::draw` (existing crate-level singletons).
- Produces: nothing new callable from outside this file -- this task wires the pool into the actual display. Not unit-testable (touches `GAME`/DOM, which are unavailable under plain `cargo test`); verified in Task 9's manual pass.

- [ ] **Step 1: Add the `Game` import**

In `src/game/learning.rs`'s import block, change:

```rust
use crate::game::structs::create_players;
```

to:

```rust
use crate::game::structs::{create_players, Game};
```

- [ ] **Step 2: Add the snapshot static and the three new functions**

Immediately after the `select_training_slot` function added in Task 2, add:

```rust
/// GAME's real field/hands/deck, saved the moment the viewer starts
/// overwriting them (see sync_training_viewer_display), so leaving the
/// viewer -- however that happens -- can put them back rather than leaving a
/// real match to start or resume from stale self-play data. GAME::new() is
/// only ever called at page load, when hosting an online match, and from the
/// GAMEOVER "Restart?" button, so nothing else would undo the overwrite
static mut GAME_SNAPSHOT_BEFORE_VIEWING: Option<(Field, Vec<Card>, Vec<Card>, Vec<Card>)> = None;

fn training_viewer_panel_visible() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("training-viewer-panel"))
        .map(|el| !el.has_attribute("hidden"))
        .unwrap_or(false)
}

/// puts GAME's pre-viewing field/hands/deck back if the viewer left one
/// behind; a no-op otherwise. Called every tick from
/// sync_training_viewer_display while the panel is hidden (self-healing
/// regardless of exit path -- "Back" button, the top "Main Menu" button, or
/// navigating to another Settings sub-panel, none of which need to know
/// anything about the viewer), and once more from
/// set_learning_mode_enabled(false) for the one path a tick can't reach:
/// learning mode turned off while the viewer panel was still open
fn restore_game_snapshot(game: &mut Game) {
    if let Some((field, player_1, player_2, deck)) = unsafe { GAME_SNAPSHOT_BEFORE_VIEWING.take() } {
        game.field = field;
        game.player_1 = player_1;
        game.player_2 = player_2;
        game.deck = deck;
        crate::render::render::draw();
    }
}

/// the one place in this module that touches GAME/DOM -- everything else here
/// is deliberately headless (see the module doc comment) so it can run from
/// cargo test and the background loop alike; this function only ever runs
/// from the real browser tick
fn sync_training_viewer_display() {
    let game = match unsafe { crate::GAME.as_mut() } {
        Some(game) => game,
        None => return,
    };

    if training_viewer_panel_visible() {
        unsafe {
            if GAME_SNAPSHOT_BEFORE_VIEWING.is_none() {
                GAME_SNAPSHOT_BEFORE_VIEWING = Some((
                    game.field.clone(),
                    game.player_1.clone(),
                    game.player_2.clone(),
                    game.deck.clone(),
                ));
            }
        }
        let slot = &training_pool()[unsafe { SELECTED_TRAINING_SLOT }];
        game.field = slot.field.clone();
        game.player_1 = slot.player_1.clone();
        game.player_2 = slot.player_2.clone();
        game.deck = slot.deck.clone();
        crate::render::render::draw();
    } else {
        restore_game_snapshot(game);
    }
}
```

- [ ] **Step 3: Wire it into the tick and into turning learning mode off**

In `self_play_tick`, add the call after `step_training_pool()`:

```rust
fn self_play_tick() {
    let decisive = run_self_play_batch(unsafe { SELF_PLAY_BATCH_SIZE });
    step_training_pool();
    sync_training_viewer_display();
    unsafe {
        GAMES_PLAYED_THIS_SESSION += decisive;
        TICKS_SINCE_SAVE += 1;
        if TICKS_SINCE_SAVE >= SAVE_EVERY_N_TICKS {
            TICKS_SINCE_SAVE = 0;
            save_persisted_table();
        }
    }
    update_progress_display();
}
```

In `set_learning_mode_enabled`, add the direct restore call in the `else` branch:

```rust
pub fn set_learning_mode_enabled(enabled: bool) {
    unsafe {
        LEARNING_MODE_ENABLED = enabled;
    }
    if enabled {
        start_self_play_loop();
    } else {
        stop_self_play_loop();
        save_persisted_table();
        if let Some(game) = unsafe { crate::GAME.as_mut() } {
            restore_game_snapshot(game);
        }
    }
    update_progress_display();
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo test`
Expected: PASS -- all 13 prior tests still pass unchanged (this task adds no new automated tests; `sync_training_viewer_display` needs a real `GAME`/DOM, so it's covered by Task 9's manual pass instead). A plain `cargo test` build confirms this compiles correctly for both the native test target and, implicitly, the shared logic the wasm target will also use.

Run: `cargo build --lib --target wasm32-unknown-unknown`
Expected: PASS -- confirms the `web_sys`/`crate::GAME`/`crate::render` calls compile cleanly for the actual WASM target too (the native `cargo test` run above doesn't exercise the wasm-only code paths at all, just that they type-check).

- [ ] **Step 5: Commit**

```bash
git add src/game/learning.rs
git commit -m "$(cat <<'EOF'
Mirror the selected training slot into GAME for display

sync_training_viewer_display reuses the live GAME singleton and
render::draw() so the training viewer looks identical to a real
match, snapshotting GAME's real field/hands/deck before overwriting
them and restoring that snapshot the moment the viewer panel becomes
hidden (whatever the reason) or learning mode is turned off mid-view --
otherwise a real match started afterward would begin from leftover
self-play data instead of a fresh or resumed board.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Guard `handle_mousedown` against non-live-match states

**Files:**
- Modify: `src/events/mousedown_handler.rs:15-19`

**Interfaces:**
- Consumes: `GameState` (existing enum from `crate::game::structs`).
- Produces: nothing new callable -- this closes the gap that would otherwise let a click on the viewer's mirrored board be processed as a real move. Not unit-testable (needs `GAME`/DOM); verified in Task 9.

- [ ] **Step 1: Add the guard**

In `src/events/mousedown_handler.rs`, the function currently starts:

```rust
pub fn handle_mousedown(str_id: String) {
    let game = unsafe { GAME.as_mut().unwrap() };
    let turn = &game.turn;
    let id = RenderId::from(str_id);
```

Change it to:

```rust
pub fn handle_mousedown(str_id: String) {
    let game = unsafe { GAME.as_mut().unwrap() };

    // outside a live match, GAME's field/hands may not even belong to a real
    // in-progress game -- eg. the training viewer (see
    // game/learning.rs::sync_training_viewer_display) mirrors self-play data
    // into GAME while GameState is SETTINGS so it can reuse this same render
    // pipeline. Nothing below this point was ever guarded against that case,
    // so ignore every click unless a real match is actually in progress
    if !matches!(game.state, GameState::PLAYAI | GameState::PLAYVS | GameState::PLAYONLINE) {
        return;
    }

    let turn = &game.turn;
    let id = RenderId::from(str_id);
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo test`
Expected: PASS -- `GameState` is already imported in this file (`use crate::game::{... structs::*};`), so no new imports are needed; this is a pure control-flow change with no new tests of its own (covered in Task 9's manual pass, since it needs a real click event and `GAME`).

- [ ] **Step 3: Commit**

```bash
git add src/events/mousedown_handler.rs
git commit -m "$(cat <<'EOF'
Guard handle_mousedown against clicks outside a live match

handle_mousedown had no top-level GameState check -- the existing
PLAYAI/PLAYONLINE handling only special-cased behaviour *within*
those states. Once the training viewer can mirror live field/hand
data into GAME while GameState is SETTINGS, a click on the visible
board would otherwise fall through into the same click-handling logic
a real match uses, mutating GAME out of sync with whatever training
slot it was mirrored from. Also closes a latent gap in the existing
code: nothing previously stopped this same class of stray click if a
field/hand were ever rendered outside a live match state.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Settings panel markup and CSS

**Files:**
- Modify: `static/index.html:44-132` (the `menu-SETTINGS` panel)
- Modify: `static/index.css` (new rules)

**Interfaces:**
- Produces: DOM elements `#settings-main-content`, `#training-viewer-panel`, `button-TRAINING_WATCH`, `button-TRAINING_BACK`, `button-TRAINING_SLOT_0`..`button-TRAINING_SLOT_9`, `input-TRAINING_BATCH_SIZE`, `input-TRAINING_TICK_MS`, plus the `viewing` CSS class target on `#menu`. Task 7 adds `data-i18n` translations for the new labels; Task 8 wires all of these elements to the Rust functions from Tasks 2-4.

- [ ] **Step 1: Wrap the existing Settings content and add the AI Learning Mode additions**

In `static/index.html`, the `menu-SETTINGS` div currently reads (abbreviated -- full content is `select-AI_DIFFICULTY` through the `button-CARDCOUNTS` button):

```html
				<div id="menu-SETTINGS" class="menu-item" hidden>
					<label class="setting-menu" for="select-AI_DIFFICULTY">
						...
					</label>
					<label class="setting-menu" for="checkbox-AI_LEARNING_MODE">
						<h3 data-i18n="settings.aiLearningMode">
							AI Learning Mode (runs background self-play games to improve the AI while
							this tab is open; progress is saved automatically)
						</h3>
						<input id="checkbox-AI_LEARNING_MODE" type="checkbox" />
						<span class="checkbox"></span>
					</label>
					<p id="learning-progress"></p>
					...
					<button class="menu-button" id="button-CARDCOUNTS" data-i18n="settings.cardCounts">
						Change Card Counts
					</button>
				</div>
```

Change it to (wrapping everything from `select-AI_DIFFICULTY` through `button-CARDCOUNTS` in `#settings-main-content`, adding the batch/tick inputs and "Watch" button right after `#learning-progress`, and adding `#training-viewer-panel` as a sibling after the wrapper, still inside `menu-SETTINGS`):

```html
				<div id="menu-SETTINGS" class="menu-item" hidden>
					<div id="settings-main-content">
						<label class="setting-menu" for="select-AI_DIFFICULTY">
							<h3 data-i18n="settings.aiDifficulty">AI Difficulty</h3>
							<select id="select-AI_DIFFICULTY">
								<option value="EASY" data-i18n="settings.aiEasy">Easy</option>
								<option value="MEDIUM" selected data-i18n="settings.aiMedium">Medium</option>
								<option value="HARD" data-i18n="settings.aiHard">Hard</option>
							</select>
						</label>
						<label class="setting-menu" for="checkbox-AI_LEARNING_MODE">
							<h3 data-i18n="settings.aiLearningMode">
								AI Learning Mode (runs background self-play games to improve the AI while
								this tab is open; progress is saved automatically)
							</h3>
							<input id="checkbox-AI_LEARNING_MODE" type="checkbox" />
							<span class="checkbox"></span>
						</label>
						<p id="learning-progress"></p>
						<label class="setting-menu" for="input-TRAINING_BATCH_SIZE">
							<h3 data-i18n="settings.trainingBatchSize">Self-play games per tick</h3>
							<input id="input-TRAINING_BATCH_SIZE" type="number" min="1" value="3" />
						</label>
						<label class="setting-menu" for="input-TRAINING_TICK_MS">
							<h3 data-i18n="settings.trainingTickMs">Self-play tick interval (ms)</h3>
							<input id="input-TRAINING_TICK_MS" type="number" min="10" value="150" />
						</label>
						<button class="menu-button" id="button-TRAINING_WATCH" data-i18n="settings.watchTraining">
							Watch Training
						</button>
						<label class="setting-menu" for="checkbox-DISPLAY_LN_FOR_LOG">
							<h3 data-i18n="settings.displayLn">Display Ln instead of Log ?</h3>
							<input id="checkbox-DISPLAY_LN_FOR_LOG" type="checkbox" />
							<span class="checkbox"></span>
						</label>
						<label class="setting-menu" for="checkbox-ALLOW_LINEAR_DEPENDENCE">
							<h3 data-i18n="settings.linearDependence">
								Allow linearly dependent field bases ? (ie. bases that are scalar multiples of
								each other, like x and 2x)
							</h3>
							<input id="checkbox-ALLOW_LINEAR_DEPENDENCE" type="checkbox" />
							<span class="checkbox"></span>
						</label>
						<label class="setting-menu" for="select-ALLOW_LIMITS_BEYOND_BOUNDS">
							<h3 data-i18n="settings.limitsBeyondBounds">
								Allow limits outside a function's domain ? (ie. arccos/arcsin are only defined
								for inputs in [-1, 1], so arccos(∞) is normally invalid)
							</h3>
							<select id="select-ALLOW_LIMITS_BEYOND_BOUNDS">
								<option value="0">Disabled</option>
								<option value="1" selected>Enabled</option>
								<option value="2">Range Selection Mode</option>
							</select>
						</label>
						<label class="setting-menu" for="select-INVERSE_TRIG_PRINCIPAL_VALUE" id="label-INVERSE_TRIG_PRINCIPAL_VALUE" hidden>
							<h3 data-i18n="settings.inverseTrigPrincipalValue">
								Principal value selection for inverse trig functions
							</h3>
							<select id="select-INVERSE_TRIG_PRINCIPAL_VALUE">
								<option value="0" selected>Standard [0, π] for arccos, [-π/2, π/2] for arcsin</option>
								<option value="1">Alternative ranges</option>
							</select>
						</label>
						<label class="setting-menu" for="checkbox-FULL_COMPUTE">
							<h3 data-i18n="settings.fullCompute">Perform all computations ?</h3>
							<input id="checkbox-FULL_COMPUTE" type="checkbox" />
							<span class="checkbox"></span>
						</label>
						<label class="setting-menu" for="checkbox-USE_FRACTIONAL_EXPONENTS">
							<h3 data-i18n="settings.fractionalExponents">
								Display roots as x^(1/2) instead of √x
							</h3>
							<input id="checkbox-USE_FRACTIONAL_EXPONENTS" type="checkbox" checked />
							<span class="checkbox"></span>
						</label>
						<label class="setting-menu" for="checkbox-LIMIT_FIELD_BASIS">
							<h3 data-i18n="settings.limitFieldBasis">Only allow max 3 field basis ?</h3>
							<input id="checkbox-LIMIT_FIELD_BASIS" type="checkbox" checked />
							<span class="checkbox"></span>
						</label>
						<label class="setting-menu" for="checkbox-CONFIRM_BEFORE_PLAY">
							<h3 data-i18n="settings.confirmBeforePlay">
								Confirm before playing a card ? (shows what the field will look like first)
							</h3>
							<input id="checkbox-CONFIRM_BEFORE_PLAY" type="checkbox" />
							<span class="checkbox"></span>
						</label>

						<label class="setting-menu" for="colour-PLAYER_1">
							<h3 data-i18n="settings.player1Colour">Player 1 Colour</h3>
							<input id="colour-PLAYER_1" type="color" value="#FF0000" />
						</label>
						<label class="setting-menu" for="colour-PLAYER_2">
							<h3 data-i18n="settings.player2Colour">Player 2 Colour</h3>
							<input id="colour-PLAYER_2" type="color" value="#0000FF" />
						</label>

						<button class="menu-button" id="button-CARDCOUNTS" data-i18n="settings.cardCounts">
							Change Card Counts
						</button>
					</div>
					<div id="training-viewer-panel" hidden>
						<div id="training-viewer-controls">
							<button class="menu-button" id="button-TRAINING_BACK" data-i18n="settings.back">
								Back
							</button>
							<div id="training-viewer-slots">
								<button class="menu-button" id="button-TRAINING_SLOT_0">1</button>
								<button class="menu-button" id="button-TRAINING_SLOT_1">2</button>
								<button class="menu-button" id="button-TRAINING_SLOT_2">3</button>
								<button class="menu-button" id="button-TRAINING_SLOT_3">4</button>
								<button class="menu-button" id="button-TRAINING_SLOT_4">5</button>
								<button class="menu-button" id="button-TRAINING_SLOT_5">6</button>
								<button class="menu-button" id="button-TRAINING_SLOT_6">7</button>
								<button class="menu-button" id="button-TRAINING_SLOT_7">8</button>
								<button class="menu-button" id="button-TRAINING_SLOT_8">9</button>
								<button class="menu-button" id="button-TRAINING_SLOT_9">10</button>
							</div>
						</div>
					</div>
				</div>
```

- [ ] **Step 2: Add the CSS**

In `static/index.css`, after the existing `#menu { ... }` rule, add:

```css
/* while the training viewer is open, #menu's own opaque background (above)
   would otherwise hide #canvas (z-index 1) and #katex (z-index 2) completely
   -- this is the only change needed to reveal them, since render::draw() and
   the KaTeX layer both already position everything at the right coordinates
   regardless of what #menu is doing */
#menu.viewing {
	background: none;
}

/* the training viewer's own controls need their own backing, now that
   #menu's background is gone while viewing */
#training-viewer-panel {
	position: relative;
	z-index: 4;
}

#training-viewer-controls {
	background-color: rgba(255, 255, 255, 0.85);
	border-radius: 5px;
	padding: 1em;
}

#training-viewer-slots {
	display: flex;
	flex-wrap: wrap;
	gap: 0.5em;
	margin-top: 1em;
}

#training-viewer-slots button {
	flex: 0 0 auto;
	margin: 0;
}

/* the title floating unbacked over a moving board reads as clutter, not a
   HUD -- hide it while viewing, same as the rest of the Settings content */
#menu.viewing > h1.title {
	display: none;
}
```

- [ ] **Step 3: Verify the dev build serves without errors**

Run: `yarn build:wasm` (compiles the Rust/WASM side; this task doesn't touch Rust, but confirms nothing else broke) then open `static/index.html`'s structure mentally against the diff above -- no automated check exists for markup/CSS correctness in this project (see spec's Testing section), so this step is a careful re-read of the diff rather than a command. Actual visual verification happens in Task 9.

- [ ] **Step 4: Commit**

```bash
git add static/index.html static/index.css
git commit -m "$(cat <<'EOF'
Add training viewer markup and CSS to the Settings panel

Wraps the existing Settings content in #settings-main-content so it
can be hidden as a block, adds a "Watch Training" button and live
batch-size/tick-interval inputs, and a #training-viewer-panel
sub-panel (10 slot-select buttons + Back) following the same nested
toggle-panel pattern menu-PLAYONLINE already uses for its Create/Join
flow. #menu.viewing removes #menu's opaque background so the real
canvas/KaTeX layers underneath become visible while the panel is open.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: i18n labels

**Files:**
- Modify: `js/i18n.js` (both the `en` and `ja` translation blocks)

**Interfaces:**
- Produces: translation keys `settings.trainingBatchSize`, `settings.trainingTickMs`, `settings.watchTraining`, `settings.back` in both languages, matching the `data-i18n` attributes added in Task 6.

- [ ] **Step 1: Add the English strings**

In `js/i18n.js`'s `en` block, right after the existing `'settings.aiLearningMode': ...` entry, add:

```js
		'settings.trainingBatchSize': 'Self-play games per tick',
		'settings.trainingTickMs': 'Self-play tick interval (ms)',
		'settings.watchTraining': 'Watch Training',
		'settings.back': 'Back',
```

- [ ] **Step 2: Add the Japanese strings**

In `js/i18n.js`'s `ja` block, right after the existing `'settings.aiLearningMode': ...` entry, add:

```js
		'settings.trainingBatchSize': 'ティックごとの自己対戦ゲーム数',
		'settings.trainingTickMs': '自己対戦のティック間隔 (ms)',
		'settings.watchTraining': '観戦する',
		'settings.back': '戻る',
```

- [ ] **Step 3: Verify the file is still valid JS**

Run: `node -e "require('./js/i18n.js')"` from the project root -- if this errors with anything other than a module-export-related message (the file may not export anything, that's fine), it means a syntax error was introduced; a clean run or a "nothing exported"-style outcome both indicate the object literal itself parsed fine.

- [ ] **Step 4: Commit**

```bash
git add js/i18n.js
git commit -m "$(cat <<'EOF'
Add EN/JA labels for the training viewer's new Settings controls

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Wire the Settings UI to the Rust functions

**Files:**
- Modify: `src/menu.rs`

**Interfaces:**
- Consumes: `crate::game::learning::{select_training_slot, set_self_play_batch_size, set_self_play_tick_ms}` (Tasks 2-3), the DOM elements from Task 6.
- Produces: a new `TrainingViewerMenu` controller struct, stored on `Menu`, constructed in `Menu::new`.

- [ ] **Step 1: Add the batch-size/tick-interval inputs to `SettingsMenu`**

In `src/menu.rs`, `SettingsMenu::new`, right after the `ai_difficulty_listener` is constructed (before the `card_counts_button` block), add:

```rust
        let training_batch_size = document
            .get_element_by_id("input-TRAINING_BATCH_SIZE")
            .unwrap();
        let training_batch_size_listener = EventListener::new(&training_batch_size, "change", |e| {
            let event_target = e.target().unwrap();
            let input = event_target.dyn_ref::<HtmlInputElement>().unwrap();
            let value: u32 = input.value().parse().unwrap_or(3);
            crate::game::learning::set_self_play_batch_size(value);
        });

        let training_tick_ms = document.get_element_by_id("input-TRAINING_TICK_MS").unwrap();
        let training_tick_ms_listener = EventListener::new(&training_tick_ms, "change", |e| {
            let event_target = e.target().unwrap();
            let input = event_target.dyn_ref::<HtmlInputElement>().unwrap();
            let value: u32 = input.value().parse().unwrap_or(150);
            crate::game::learning::set_self_play_tick_ms(value);
        });
```

Add the four new fields to the `SettingsMenu` struct definition. It currently reads:

```rust
pub struct SettingsMenu {
    checkboxes: Vec<Element>,
    checkbox_listeners: HashMap<String, EventListener>,

    colours: Vec<Element>,
    colour_listeners: HashMap<String, EventListener>,

    ai_difficulty: Element,
    ai_difficulty_listener: EventListener,

    card_counts_button: Element,
    card_counts_button_listener: EventListener,
    card_counts: Vec<Element>,
    card_count_listeners: HashMap<String, EventListener>,
    reset_card_counts_button: Element,
    reset_card_counts_listener: EventListener,
}
```

Change it to:

```rust
pub struct SettingsMenu {
    checkboxes: Vec<Element>,
    checkbox_listeners: HashMap<String, EventListener>,

    colours: Vec<Element>,
    colour_listeners: HashMap<String, EventListener>,

    ai_difficulty: Element,
    ai_difficulty_listener: EventListener,

    training_batch_size: Element,
    training_batch_size_listener: EventListener,
    training_tick_ms: Element,
    training_tick_ms_listener: EventListener,

    card_counts_button: Element,
    card_counts_button_listener: EventListener,
    card_counts: Vec<Element>,
    card_count_listeners: HashMap<String, EventListener>,
    reset_card_counts_button: Element,
    reset_card_counts_listener: EventListener,
}
```

And the `Self { ... }` construction at the end of `SettingsMenu::new` currently reads:

```rust
        Self {
            checkboxes,
            checkbox_listeners,
            colours,
            colour_listeners,
            ai_difficulty,
            ai_difficulty_listener,
            card_counts_button,
            card_counts_button_listener,
            card_counts,
            card_count_listeners,
            reset_card_counts_button,
            reset_card_counts_listener,
        }
```

Change it to:

```rust
        Self {
            checkboxes,
            checkbox_listeners,
            colours,
            colour_listeners,
            ai_difficulty,
            ai_difficulty_listener,
            training_batch_size,
            training_batch_size_listener,
            training_tick_ms,
            training_tick_ms_listener,
            card_counts_button,
            card_counts_button_listener,
            card_counts,
            card_count_listeners,
            reset_card_counts_button,
            reset_card_counts_listener,
        }
```

- [ ] **Step 2: Add the `TrainingViewerMenu` controller**

At the end of `src/menu.rs`, after the closing brace of `impl OnlineMenu`, add:

```rust
/// controller for the training viewer sub-panel inside Settings
#[allow(dead_code)]
pub struct TrainingViewerMenu {
    watch_button: Element,
    watch_listener: EventListener,
    back_button: Element,
    back_listener: EventListener,
    slot_buttons: Vec<Element>,
    slot_listeners: HashMap<String, EventListener>,
}

impl TrainingViewerMenu {
    pub fn new(document: &Document) -> Self {
        let menu_element = document.get_element_by_id("menu").unwrap();
        let settings_main_content = document.get_element_by_id("settings-main-content").unwrap();
        let viewer_panel = document.get_element_by_id("training-viewer-panel").unwrap();

        let watch_button = document.get_element_by_id("button-TRAINING_WATCH").unwrap();
        let watch_listener = {
            let menu_element = menu_element.clone();
            let settings_main_content = settings_main_content.clone();
            let viewer_panel = viewer_panel.clone();
            EventListener::new(&watch_button, "click", move |_e| {
                // #menu has no other class today, so a plain set/remove of the
                // whole `class` attribute is simplest -- no need to pull in
                // Element::class_list() (a separate web-sys feature) just to
                // toggle one flag
                menu_element.set_attribute("class", "viewing").ok();
                settings_main_content.set_attribute("hidden", "true").ok();
                viewer_panel.remove_attribute("hidden").ok();
            })
        };

        let back_button = document.get_element_by_id("button-TRAINING_BACK").unwrap();
        let back_listener = {
            let menu_element = menu_element.clone();
            let settings_main_content = settings_main_content.clone();
            let viewer_panel = viewer_panel.clone();
            EventListener::new(&back_button, "click", move |_e| {
                menu_element.remove_attribute("class").ok();
                viewer_panel.set_attribute("hidden", "true").ok();
                settings_main_content.remove_attribute("hidden").ok();
            })
        };

        let slot_buttons: Vec<Element> = (0..10)
            .map(|i| {
                document
                    .get_element_by_id(&format!("button-TRAINING_SLOT_{}", i))
                    .unwrap()
            })
            .collect();

        let mut slot_listeners: HashMap<String, EventListener> = HashMap::new();
        for (i, button) in slot_buttons.iter().enumerate() {
            let listener = EventListener::new(button, "click", move |_e| {
                crate::game::learning::select_training_slot(i);
            });
            slot_listeners.insert(button.id(), listener);
        }

        Self {
            watch_button,
            watch_listener,
            back_button,
            back_listener,
            slot_buttons,
            slot_listeners,
        }
    }
}
```

- [ ] **Step 3: Wire `TrainingViewerMenu` into `Menu`**

In `src/menu.rs`, add `pub training_viewer_menu: TrainingViewerMenu,` to the `Menu` struct's field list (alongside `pub online_menu: OnlineMenu,`), and in `Menu::new`, right after `let online_menu = OnlineMenu::new(document);`, add:

```rust
        let training_viewer_menu = TrainingViewerMenu::new(document);
```

Then add `training_viewer_menu,` to the `Menu { ... }` construction at the end of `Menu::new`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo build --lib --target wasm32-unknown-unknown`
Expected: PASS. Also run `cargo test` to confirm the native test target still compiles and all prior tests pass (Rust compiles this file for both targets; `web_sys`/`gloo` types used here are the same ones the rest of `menu.rs` already uses successfully under both).

- [ ] **Step 5: Commit**

```bash
git add src/menu.rs
git commit -m "$(cat <<'EOF'
Wire the training viewer's Settings controls to learning.rs

TrainingViewerMenu handles Watch/Back (setting/removing #menu's
"viewing" class attribute and swapping #settings-main-content for
#training-viewer-panel) and the 10 slot-select buttons (calling
select_training_slot). The two new number inputs in SettingsMenu call
set_self_play_batch_size/set_self_play_tick_ms on change, following
the existing checkbox/select wiring pattern in this file.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Manual browser verification

**Files:** none (no code changes -- this task is a verification pass over Tasks 1-8's combined result, per the spec's Testing section, matching this project's existing convention of manual/dev-build verification for anything DOM-dependent)

- [ ] **Step 1: Start the dev server**

Run: `yarn start`
Expected: builds successfully and opens the game in a browser at the dev server URL, with live-reload on `src` changes.

- [ ] **Step 2: Enable AI Learning Mode and open the viewer**

In the browser: Main Menu -> Settings -> check "AI Learning Mode" -> confirm `#learning-progress` starts showing a pattern/game count that increases over a few seconds -> click "Watch Training".
Expected: the Settings list disappears, a live game board appears (using the same card art/layout as a real match) with 10 numbered slot buttons and a "Back" button, and the board visibly changes rapidly (cards appearing/clearing) without any deliberate slow-motion pacing.

- [ ] **Step 3: Switch between slots**

Click a few different "Game N" buttons.
Expected: the displayed board changes to a different, independently-progressing game each time; the previously-viewed slot keeps advancing in the background (confirm by switching back to it -- it should have visibly changed further, not be paused where you left it).

- [ ] **Step 4: Confirm the board is read-only**

While viewing, click directly on a few cards/field slots on the displayed board.
Expected: nothing happens -- no card selection highlight, no phase change, no console error. (This exercises the `handle_mousedown` guard from Task 5.)

- [ ] **Step 5: Confirm exit paths restore real game state, via "Back"**

Click "Back".
Expected: returns to the normal Settings list; the canvas area is no longer visible (covered by `#menu` again). Navigate to "Play vs AI" from the main menu and confirm the match starts from a normal fresh board (starting field + 7-card hands), not from whatever training board was last displayed.

- [ ] **Step 6: Confirm exit paths restore real game state, via "Main Menu"**

Repeat: Settings -> AI Learning Mode -> Watch Training -> watch for a few seconds -> click the top-left "Main Menu" button directly (skipping "Back") -> Play vs AI.
Expected: same result as Step 5 -- a normal fresh board, confirming the tick-driven self-healing restore (not just the "Back" button's own handler) is what's doing the work.

- [ ] **Step 7: Note the one exit path that can't be exercised through the UI**

`set_learning_mode_enabled(false)`'s direct restore call (Task 4, Step 3) exists for "learning mode turned off while the viewer panel is still open" -- but the "AI Learning Mode" checkbox lives inside `#settings-main-content`, which Task 6's markup hides (`hidden` attribute, not just visually) for as long as `#training-viewer-panel` is showing. The checkbox is therefore never reachable while the viewer is open, through any normal interaction, and `set_learning_mode_enabled` isn't exposed to the JS console (no `#[wasm_bindgen]`), so this specific branch can't be triggered from the browser at all in the shipped UI. This is expected -- it's a defensive-correctness path for robustness against future UI changes, not a reachable user flow. No action needed here beyond noting it in the final report; do not modify the UI to force this path reachable.

- [ ] **Step 8: Try the batch size / tick interval inputs**

In Settings (with AI Learning Mode on, viewer closed), change "Self-play games per tick" and "Self-play tick interval (ms)" to a few different values, including `0` in each.
Expected: `0` inputs don't break anything (floored to 1 / 10ms per Task 3); non-zero changes are visibly reflected in how fast `#learning-progress`'s counters climb.

- [ ] **Step 9: Run the full automated suite one more time**

Run: `yarn test:cargo`
Expected: PASS, all tests from Tasks 1-3 (13 in `learning.rs` alone) plus every pre-existing test in the project.

- [ ] **Step 10: Report results**

Summarize which steps passed as expected, and paste the exact wording of any step that didn't (screenshot if visual) rather than paraphrasing, so a follow-up fix can target the right thing precisely.
