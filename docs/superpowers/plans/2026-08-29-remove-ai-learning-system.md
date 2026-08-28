# Remove AI Self-Play Learning System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the background self-play reinforcement-learning system (`src/game/learning.rs`, `js/learning.js`, and every call site that feeds or depends on it) with no behavior change to the AI's surviving heuristic, and retire the training-viewer spec/plan docs that only made sense on top of the system being removed.

**Architecture:** This is a single cohesive removal, not a multi-step build: every edit across the ~10 touched files depends on every other one to leave the tree compiling, so there is one task covering the whole removal, ending in a full green `cargo test` run. No new runtime behavior is introduced — `choose_move`'s hard filters and lookahead scoring are untouched; only the statistical nudge layered on top of them, and everything that fed it, goes away.

**Tech Stack:** Rust/WASM, no new dependencies (removing one: the `js/learning.js` wasm-bindgen snippet and its IndexedDB usage).

**Spec:** `docs/superpowers/specs/2026-08-29-remove-ai-learning-system-design.md`

## Global Constraints

- Full removal, not a toggle-off or feature flag — no dead code, no orphaned `#[allow(dead_code)]`.
- No behavior change to the AI's surviving heuristic (hard filters, lookahead, scoring functions) — this task removes the learned-bias nudge and everything that fed it, nothing else.
- `choose_move`'s `mover`/`ai_hand` parameters must be dropped, not left unused, once the `apply_learned_bias` call that was their only use is gone.
- `AiMove::consumed_hand_indices` must be dropped too, for the same reason: a full-tree search (`grep -rn "consumed_hand_indices" src/`) confirms its only reader was `learning.rs`'s self-play loop -- everywhere else it's constructed but never read.
- No new data structure, persistence, or documentation is introduced by this task — the spec's "new practice" (regression tests per reported loss) is a future, one-at-a-time activity, out of scope for this plan's own deliverable.

---

## Task 1: Remove the self-play learning system and its call sites

**Files:**
- Delete: `src/game/learning.rs`
- Delete: `js/learning.js`
- Delete: `docs/superpowers/specs/2026-08-28-training-viewer-design.md`
- Delete: `docs/superpowers/plans/2026-08-28-training-viewer.md`
- Modify: `src/game/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/game/ai.rs`
- Modify: `src/game/structs.rs`
- Modify: `src/events/mousedown_handler.rs`
- Modify: `src/menu.rs`
- Modify: `static/index.html`
- Modify: `js/i18n.js`

**Interfaces:** None produced or consumed — this task has no downstream tasks in this plan. The only "interface" concern is that `choose_move`'s public-to-the-crate signature changes (drops 2 params); this task updates every call site of that signature within the same commit, so nothing is left broken.

- [ ] **Step 1: Delete the two learning-system files**

```bash
git rm src/game/learning.rs js/learning.js
```

- [ ] **Step 2: Remove the module declaration**

In `src/game/mod.rs`, currently:

```rust
pub mod ai;
pub mod card_counts;
pub mod card_encoding;
pub mod cards;
pub mod field;
pub mod flags;
pub mod learning;
pub mod online;
pub mod structs;
```

Remove the `pub mod learning;` line:

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

- [ ] **Step 3: Remove the startup call in `lib.rs`**

In `src/lib.rs`'s `main_js`, currently:

```rust
    unsafe {
        GAME = Some(Game::new());
        CANVAS = Some(Canvas::new(&document));
        MENU = Some(Menu::new(&document));
    }
    game::learning::load_persisted_table();
    let canvas = unsafe { CANVAS.as_mut().unwrap() };
```

Remove the `game::learning::load_persisted_table();` line:

```rust
    unsafe {
        GAME = Some(Game::new());
        CANVAS = Some(Canvas::new(&document));
        MENU = Some(Menu::new(&document));
    }
    let canvas = unsafe { CANVAS.as_mut().unwrap() };
```

- [ ] **Step 4: `src/game/ai.rs` — remove the import and the call**

Remove this line near the top of the file:

```rust
use crate::game::learning::apply_learned_bias;
```

In `choose_move`, currently:

```rust
    let mut candidates = match difficulty {
        AiDifficulty::Hard => apply_lookahead(candidates, opponent_hand, turn_number, usize::MAX),
        AiDifficulty::Medium => {
            apply_lookahead(candidates, opponent_hand, turn_number, MEDIUM_LOOKAHEAD_POOL)
        }
        AiDifficulty::Easy => candidates,
    };
    candidates = apply_learned_bias(candidates, ai_hand, field, mover, turn_number);
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
```

Remove the `apply_learned_bias` line (the following `sort_by` now operates directly on what the difficulty match produced):

```rust
    let mut candidates = match difficulty {
        AiDifficulty::Hard => apply_lookahead(candidates, opponent_hand, turn_number, usize::MAX),
        AiDifficulty::Medium => {
            apply_lookahead(candidates, opponent_hand, turn_number, MEDIUM_LOOKAHEAD_POOL)
        }
        AiDifficulty::Easy => candidates,
    };
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
```

- [ ] **Step 5: `src/game/ai.rs` — simplify `choose_move`'s signature**

`mover` and `ai_hand` were only ever used by the `apply_learned_bias` call just removed — verify this yourself by searching the function body for both names before proceeding (there should be no remaining use besides the parameter declaration).

Currently:

```rust
pub(super) fn choose_move(
    candidates: Vec<AiMove>,
    mover: u32,
    ai_hand: &[Card],
    opponent_hand: &[Card],
    difficulty: AiDifficulty,
    turn_number: u32,
    field: &Field,
) -> AiMove {
```

Change to:

```rust
pub(super) fn choose_move(
    candidates: Vec<AiMove>,
    opponent_hand: &[Card],
    difficulty: AiDifficulty,
    turn_number: u32,
    field: &Field,
) -> AiMove {
```

- [ ] **Step 6: `src/game/ai.rs` — remove the now-dead `AiMove::consumed_hand_indices` field**

This field's only reader was `learning.rs`'s self-play loop (deleted in Step 1) -- confirmed by `grep -rn "consumed_hand_indices" src/` before writing this step: every other site constructs it, nothing else reads it. Leaving it would mean 5 struct-literal sites building a `Vec` that's now permanently ignored.

Remove the field and its doc comment from the `AiMove` struct definition. Currently:

```rust
    /// hand indices this move consumes (the operator card, plus a BasisCard
    /// operand for a field+hand-card Mult/Div play) -- same indices already
    /// computed for flexibility_bonus at each construction site below, kept here
    /// too so a headless game loop (see learning::simulate_self_play_game) can
    /// remove the right cards from hand without re-deriving them from `clicks`
    pub(super) consumed_hand_indices: Vec<usize>,
}
```

Change to just:

```rust
}
```

Then remove the `consumed_hand_indices: vec![...]` line from each of the 5 `AiMove { ... }` construction sites in this file (each is currently the last field before the struct literal's closing `});`, so removing the line leaves a valid trailing comma on the field above it). Line numbers below are as of the current, unmodified file (before any of this task's earlier steps shifted anything in this file):

**Line 437** (basis-card-to-empty-slot placement), currently:

```rust
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={target}"))],
                            score: score_replacement(player_num, target, &new_basis, field)
                                + strategic_slot_bonus(player_num, &resulting_field)
                                + situation_bonus
                                + flexibility_bonus(hand, &[i]),
                            resulting_field,
                            wins_immediately,
                            hurts_self_or_helps_opponent,
                            consumed_hand_indices: vec![i],
                        });
```

becomes:

```rust
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={target}"))],
                            score: score_replacement(player_num, target, &new_basis, field)
                                + strategic_slot_bonus(player_num, &resulting_field)
                                + situation_bonus
                                + flexibility_bonus(hand, &[i]),
                            resulting_field,
                            wins_immediately,
                            hurts_self_or_helps_opponent,
                        });
```

**Line 457** (Nabla half-derivative) and **line 476** (Laplacian half-derivative, the identical pattern immediately below it) both currently end their `AiMove { ... }` literal with:

```rust
                        resulting_field,
                        wins_immediately,
                        hurts_self_or_helps_opponent,
                        consumed_hand_indices: vec![i],
                    });
```

At both locations, change to:

```rust
                        resulting_field,
                        wins_immediately,
                        hurts_self_or_helps_opponent,
                    });
```

**Line 546** (Mult/Div two-operand play), currently:

```rust
                            moves.push(AiMove {
                                clicks,
                                score,
                                resulting_field,
                                wins_immediately,
                                hurts_self_or_helps_opponent,
                                consumed_hand_indices: vec![i, hand_index],
                            });
```

becomes:

```rust
                            moves.push(AiMove {
                                clicks,
                                score,
                                resulting_field,
                                wins_immediately,
                                hurts_self_or_helps_opponent,
                            });
```

**Line 588** (single-operand replacement with a limit-card bonus), currently:

```rust
                            score: score_replacement(player_num, target, &result, field)
                                + strategic_slot_bonus(player_num, &resulting_field)
                                + limit_bonus
                                + situation_bonus
                                + flexibility_bonus(hand, &[i]),
                            resulting_field,
                            wins_immediately,
                            hurts_self_or_helps_opponent,
                            consumed_hand_indices: vec![i],
                        });
```

becomes:

```rust
                            score: score_replacement(player_num, target, &result, field)
                                + strategic_slot_bonus(player_num, &resulting_field)
                                + limit_bonus
                                + situation_bonus
                                + flexibility_bonus(hand, &[i]),
                            resulting_field,
                            wins_immediately,
                            hurts_self_or_helps_opponent,
                        });
```

That's all 5 sites (437, 457, 476, 546, 588) -- confirm no others remain with `grep -n "consumed_hand_indices" src/game/ai.rs` returning nothing before moving on.

- [ ] **Step 7: `src/game/ai.rs` — update every call site of `choose_move`**

Seven of the nine call sites share this exact single-line pattern; replace all seven occurrences of

```
choose_move(candidates, AI_PLAYER_NUM, &ai_hand, &opponent_hand,
```

with

```
choose_move(candidates, &opponent_hand,
```

(everything after `&opponent_hand,` on each of those seven lines — the difficulty, turn number, and `&field` argument — stays exactly as it already is). These seven lines currently contain (search for `choose_move(candidates, AI_PLAYER_NUM, &ai_hand, &opponent_hand, AiDifficulty::Hard, ` to find all seven):
- `let _chosen = choose_move(candidates, AI_PLAYER_NUM, &ai_hand, &opponent_hand, AiDifficulty::Hard, 20, &field);`
- `let chosen = choose_move(candidates, AI_PLAYER_NUM, &ai_hand, &opponent_hand, AiDifficulty::Hard, 4, &field);` (this exact line appears 6 times, at different line numbers, inside different test functions — replace every occurrence)

The remaining two call sites are multi-line and need individual edits.

Production call site (the function that drives the AI's real turn), currently:

```rust
    let difficulty = unsafe { AI_DIFFICULTY };
    let chosen = choose_move(
        candidates,
        AI_PLAYER_NUM,
        &game.player_2,
        &game.player_1,
        difficulty,
        game.turn.number,
        &game.field,
    );
```

Change to:

```rust
    let difficulty = unsafe { AI_DIFFICULTY };
    let chosen = choose_move(
        candidates,
        &game.player_1,
        difficulty,
        game.turn.number,
        &game.field,
    );
```

The `perf_tests` multi-line call site (inside a nested loop over difficulties), currently:

```rust
                let chosen = choose_move(
                    candidates_for_difficulty,
                    AI_PLAYER_NUM,
                    &ai_hand,
                    &opponent_hand,
                    difficulty,
                    turn_number,
                    &field,
                );
```

Change to:

```rust
                let chosen = choose_move(
                    candidates_for_difficulty,
                    &opponent_hand,
                    difficulty,
                    turn_number,
                    &field,
                );
```

- [ ] **Step 8: `src/game/ai.rs` — update the three stale doc comments**

`choose_move`'s own doc comment currently ends:

```rust
/// beatable. All three then get a further nudge from apply_learned_bias -- see
/// its own doc -- so the AI plays somewhat better the more self-play and real
/// games it accumulates, without needing an expensive real-time search to do it.
/// Deliberately fast (no recursive search): a slow decision reads as frozen to a
/// human waiting on it, and this game's actual strength should come from what's
/// been learned, not from how long a single decision is allowed to take
```

Change to:

```rust
/// beatable. This heuristic is hand-tuned, not learned: each concrete mistake
/// found in real play becomes a regression test in perf_tests (see that
/// module), with the responsible rule fixed until the test passes.
/// Deliberately fast (no recursive search): a slow decision reads as frozen to a
/// human waiting on it
```

The comment on `test_hard_ai_decision_completes_quickly_in_worst_case` currently reads:

```rust
    /// reproduces the "AI stops" report: both the AI's and opponent's hands stacked
    /// with the most expensive card type combinations (Mult/Div, both against
    /// other field slots and against BasisCards from hand) -- the actual worst
    /// case the real game can produce for candidate generation cost. Since
    /// choose_move no longer does any recursive search (see its doc), this is
    /// really a check on generate_candidates_for/apply_lookahead/
    /// apply_learned_bias's own cost, not on any search budget
```

Change the last two lines to:

```rust
    /// reproduces the "AI stops" report: both the AI's and opponent's hands stacked
    /// with the most expensive card type combinations (Mult/Div, both against
    /// other field slots and against BasisCards from hand) -- the actual worst
    /// case the real game can produce for candidate generation cost. Since
    /// choose_move no longer does any recursive search (see its doc), this is
    /// really a check on generate_candidates_for/apply_lookahead's own cost,
    /// not on any search budget
```

The comment on `test_ai_never_hurts_itself_when_a_fully_safe_alternative_exists` currently reads:

```rust
    /// This property is guaranteed by apply_hard_filters before any
    /// difficulty-specific ranking (apply_lookahead/apply_learned_bias) ever
    /// runs, so it holds regardless of that ranking's own logic
```

Change to:

```rust
    /// This property is guaranteed by apply_hard_filters before any
    /// difficulty-specific ranking (apply_lookahead) ever runs, so it holds
    /// regardless of that ranking's own logic
```

- [ ] **Step 9: `src/game/structs.rs` — remove the three call sites**

In `Game::new()`, currently:

```rust
    pub fn new() -> Game {
        let mut deck = get_new_deck();
        deck.shuffle(&mut thread_rng());

        let (player_1, player_2) = create_players(&mut deck);
        super::learning::reset_game_log();
        return Game {
```

Remove the `reset_game_log` line:

```rust
    pub fn new() -> Game {
        let mut deck = get_new_deck();
        deck.shuffle(&mut thread_rng());

        let (player_1, player_2) = create_players(&mut deck);
        return Game {
```

In `Game::from_online_parts`, currently:

```rust
    pub fn from_online_parts(deck: Vec<Card>, player_1: Vec<Card>, player_2: Vec<Card>) -> Game {
        super::learning::reset_game_log();
        Game {
```

Remove the `reset_game_log` line:

```rust
    pub fn from_online_parts(deck: Vec<Card>, player_1: Vec<Card>, player_2: Vec<Card>) -> Game {
        Game {
```

In `Game::game_over`, currently:

```rust
    pub fn game_over(&self, winner: u32) {
        super::learning::finish_game_and_learn(winner);

        let menu = unsafe { MENU.as_ref().unwrap() };
```

Remove the `finish_game_and_learn` call and the blank line after it:

```rust
    pub fn game_over(&self, winner: u32) {
        let menu = unsafe { MENU.as_ref().unwrap() };
```

- [ ] **Step 10: `src/events/mousedown_handler.rs` — remove the real-move recording block**

In `end_turn`, currently:

```rust
/// performs cleanup tasks after turn is over
fn end_turn(old_field: Field) {
    let game = unsafe { GAME.as_mut().unwrap() };

    // feed the learning system with what just happened, before this turn's
    // card removal/redraw below changes the hand out from under it -- gated to
    // PLAYAI since that's the only mode where the AI is actually a participant
    // (see finish_game_and_learn's own doc for why PLAYVS/PLAYONLINE don't
    // need a matching gate on the other end)
    if matches!(game.state, GameState::PLAYAI) {
        let mover = game.get_current_player_num();
        let hand = if mover == 1 { &game.player_1 } else { &game.player_2 };
        crate::game::learning::record_real_move(&old_field, &game.field, mover, game.turn.number, hand);
    }

    // get vector indices of cards used by player this turn
    let mut selected_indices = game
```

Remove the comment and the `if matches!` block:

```rust
/// performs cleanup tasks after turn is over
fn end_turn(old_field: Field) {
    let game = unsafe { GAME.as_mut().unwrap() };

    // get vector indices of cards used by player this turn
    let mut selected_indices = game
```

- [ ] **Step 11: `src/menu.rs` — remove the checkbox wiring**

In `SettingsMenu::new`, currently:

```rust
        let checkboxes: Vec<Element> = vec![
            "DISPLAY_LN_FOR_LOG",
            "ALLOW_LINEAR_DEPENDENCE",
            "FULL_COMPUTE",
            "USE_FRACTIONAL_EXPONENTS",
            "LIMIT_FIELD_BASIS",
            "CONFIRM_BEFORE_PLAY",
            "AI_LEARNING_MODE",
        ]
```

Remove `"AI_LEARNING_MODE",`:

```rust
        let checkboxes: Vec<Element> = vec![
            "DISPLAY_LN_FOR_LOG",
            "ALLOW_LINEAR_DEPENDENCE",
            "FULL_COMPUTE",
            "USE_FRACTIONAL_EXPONENTS",
            "LIMIT_FIELD_BASIS",
            "CONFIRM_BEFORE_PLAY",
        ]
```

Further down, in the same function's checkbox `match`, currently:

```rust
                unsafe {
                    match flag_name {
                        "DISPLAY_LN_FOR_LOG" => DISPLAY_LN_FOR_LOG = flag_value,
                        "ALLOW_LINEAR_DEPENDENCE" => ALLOW_LINEAR_DEPENDENCE = flag_value,
                        "FULL_COMPUTE" => FULL_COMPUTE = flag_value,
                        "USE_FRACTIONAL_EXPONENTS" => USE_FRACTIONAL_EXPONENTS = flag_value,
                        "LIMIT_FIELD_BASIS" => LIMIT_FIELD_BASIS = flag_value,
                        "CONFIRM_BEFORE_PLAY" => CONFIRM_BEFORE_PLAY = flag_value,
                        "AI_LEARNING_MODE" => {
                            crate::game::learning::set_learning_mode_enabled(flag_value)
                        }
                        _ => panic!("Unknown flag name: {}", flag_name),
                    }
                }
```

Remove the `"AI_LEARNING_MODE"` arm:

```rust
                unsafe {
                    match flag_name {
                        "DISPLAY_LN_FOR_LOG" => DISPLAY_LN_FOR_LOG = flag_value,
                        "ALLOW_LINEAR_DEPENDENCE" => ALLOW_LINEAR_DEPENDENCE = flag_value,
                        "FULL_COMPUTE" => FULL_COMPUTE = flag_value,
                        "USE_FRACTIONAL_EXPONENTS" => USE_FRACTIONAL_EXPONENTS = flag_value,
                        "LIMIT_FIELD_BASIS" => LIMIT_FIELD_BASIS = flag_value,
                        "CONFIRM_BEFORE_PLAY" => CONFIRM_BEFORE_PLAY = flag_value,
                        _ => panic!("Unknown flag name: {}", flag_name),
                    }
                }
```

- [ ] **Step 12: `static/index.html` — remove the Settings UI**

In the `menu-SETTINGS` panel, currently:

```html
					<label class="setting-menu" for="checkbox-AI_LEARNING_MODE">
						<h3 data-i18n="settings.aiLearningMode">
							AI Learning Mode (runs background self-play games to improve the AI while
							this tab is open; progress is saved automatically)
						</h3>
						<input id="checkbox-AI_LEARNING_MODE" type="checkbox" />
						<span class="checkbox"></span>
					</label>
					<p id="learning-progress"></p>
					<label class="setting-menu" for="checkbox-DISPLAY_LN_FOR_LOG">
```

Remove the `AI_LEARNING_MODE` label and the `learning-progress` paragraph:

```html
					<label class="setting-menu" for="checkbox-DISPLAY_LN_FOR_LOG">
```

- [ ] **Step 13: `js/i18n.js` — remove both translation entries**

In the `en` block, currently:

```js
		'settings.aiLearningMode':
			'AI Learning Mode (runs background self-play games to improve the AI while this tab is open; progress is saved automatically)',
		'menu.tutorial': 'Instructions',
```

Remove the `settings.aiLearningMode` entry:

```js
		'menu.tutorial': 'Instructions',
```

In the `ja` block, currently:

```js
		'settings.aiLearningMode':
			'AI学習モード（このタブを開いている間、バックグラウンドで自己対戦を行いAIを強化します。学習内容は自動的に保存されます）',
		'menu.tutorial': '遊び方',
```

Remove the `settings.aiLearningMode` entry:

```js
		'menu.tutorial': '遊び方',
```

- [ ] **Step 14: Delete the retired training-viewer spec/plan docs**

```bash
git rm docs/superpowers/specs/2026-08-28-training-viewer-design.md docs/superpowers/plans/2026-08-28-training-viewer.md
```

- [ ] **Step 15: Verify everything compiles and the full test suite is green**

Run: `cargo test`
Expected: PASS. The prior baseline (before this task) was `game::learning::tests` (12 tests) plus every other module's tests, with one known **pre-existing, unrelated** failure: `tests/basis.rs::test_complex_special_coefficients`. After this task, `game::learning::tests` no longer exists (the whole module is gone), every remaining test must still pass unchanged, and that one pre-existing `tests/basis.rs` failure is still expected (do not attempt to fix it — out of scope for this task, same as it was out of scope for the training-viewer plan this replaces).

Also run: `cargo build --lib --target wasm32-unknown-unknown`
Expected: PASS — confirms the WASM target (which the native `cargo test` run doesn't exercise) still compiles cleanly, since this change touches `wasm_bindgen`-adjacent code (`lib.rs`, `menu.rs`) that only the wasm target fully type-checks against `web-sys`.

- [ ] **Step 16: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
Remove the AI self-play learning system

Removes src/game/learning.rs and js/learning.js in full -- the
headless self-play simulation, the statistical (state, action) ->
win-rate table, the background training loop, IndexedDB persistence,
and the "AI Learning Mode" Settings toggle -- along with every call
site that fed or depended on it. choose_move's mover/ai_hand
parameters and AiMove's consumed_hand_indices field are dropped too,
now that apply_learned_bias and the self-play loop (their only uses)
are gone; the hard-filter/lookahead heuristic itself is unchanged.

Going forward, AI strength improvements come from a manual practice
instead: a reported loss becomes a new regression test in
ai::perf_tests, fixed one heuristic rule at a time, rather than from
statistical self-play. The training-viewer spec/plan (designed to
watch this system train) is retired along with it.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```
