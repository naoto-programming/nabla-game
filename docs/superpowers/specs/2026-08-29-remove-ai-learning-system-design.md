# Remove AI Self-Play Learning System Design

## Summary

Remove the background self-play reinforcement-learning system added in
"Replace minimax with a persistent self-play learning system" (commit
`e8ee1ae`) entirely: the headless self-play simulation, the statistical
(state, action) -> win-rate table, the background training loop, IndexedDB
persistence, and the "AI Learning Mode" Settings toggle. Going forward, AI
strength improvements come from a manual practice instead of an automatic
one: when a real loss to the AI reveals a bad decision, that exact
situation becomes a new regression test in the existing `ai::perf_tests`
module, and the corresponding heuristic in `choose_move` (or whichever
scoring function is responsible) is fixed until the test passes.

## Decisions made with the user

- **Full removal, not a toggle-off**: no dead code, no feature flag, no
  behind-a-flag remnant of the old system.
- **The abandoned "training viewer" work is deleted too**: that feature
  (mid-implementation via SDD, now discarded) existed specifically to
  watch this self-play system train; its design docs
  (`docs/superpowers/specs/2026-08-28-training-viewer-design.md`,
  `docs/superpowers/plans/2026-08-28-training-viewer.md`) are removed as
  part of this change rather than left as stale references to a system
  that no longer exists.
- **The "new mechanism" is not a new data structure or runtime system.**
  It reuses the existing `ai.rs::perf_tests` test module (`#[cfg(test)]`),
  already home to exactly this kind of test --
  e.g. `test_ai_avoids_immediate_loss_when_a_safe_alternative_exists`,
  `test_ai_never_picks_a_self_defeating_move_when_avoidable` assert the AI
  makes a specific correct decision in a specific engineered scenario.
  Extending this pattern needs no new module, file, or abstraction.
- **Workflow** (established as a practice, not built as software): the
  user loses a PLAYAI game -> describes the board/hand/AI's actual (bad)
  move to Claude -> Claude reproduces that exact situation as a new
  failing test in `perf_tests` (RED) -> fixes the relevant heuristic --
  a hard filter, `score_replacement`, `evaluate_game_situation`, or
  similar -- until the test passes (GREEN) -> commit. Each fixed loss
  becomes a permanent regression test, so the same mistake can't silently
  return later.
- **No formal documentation of this workflow** (no CLAUDE.md entry, no
  `docs/` note) -- by explicit user decision, it gets restated each
  session rather than relied on as written-down process.
- **Previously-learned data is simply abandoned**: any IndexedDB
  `nabla-learning` object store already sitting in a user's browser from
  prior sessions becomes orphaned, unused storage once this ships -- no
  migration, no cleanup code. It's inert once nothing reads or writes it.

## Files removed

- `src/game/learning.rs`
- `js/learning.js`
- `docs/superpowers/specs/2026-08-28-training-viewer-design.md`
- `docs/superpowers/plans/2026-08-28-training-viewer.md`

## Files changed

- **`src/game/mod.rs`** -- remove `pub mod learning;`.
- **`src/lib.rs`** -- remove the `game::learning::load_persisted_table();`
  call in `main_js` (currently line 51).
- **`src/game/ai.rs`**:
  - Remove `use crate::game::learning::apply_learned_bias;` (line 11).
  - In `choose_move`, remove
    `candidates = apply_learned_bias(candidates, ai_hand, field, mover, turn_number);`
    (line 976); `candidates.sort_by(...)` on the next line now operates
    directly on whatever `apply_lookahead`/the difficulty match produced.
  - Simplify `choose_move`'s signature: drop the `mover: u32` and
    `ai_hand: &[Card]` parameters. Verified (by reading the full function
    body) that removing the `apply_learned_bias` call leaves both
    parameters completely unused elsewhere in the function -- keeping them
    would mean dead parameters and compiler warnings.
  - Update all 9 call sites accordingly, dropping the two corresponding
    positional arguments at each: the one production call site (in the
    function that drives the AI's real turn, ~line 150) and 8 call sites
    inside `perf_tests` (~lines 1131, 1158, 1198, 1234, 1269, 1308, 1400,
    1569).
  - Update 3 stale doc comments that reference `apply_learned_bias` or
    "self-play"/"learned" nudging, since the behavior they describe no
    longer exists: `choose_move`'s own doc comment (~line 949, the
    "All three then get a further nudge from apply_learned_bias..."
    sentence), a comment on `test_hard_ai_decision_completes_quickly_in_worst_case`
    (~line 1147, "...apply_learned_bias's own cost..."), and a comment on
    `test_ai_never_hurts_itself_when_a_fully_safe_alternative_exists`
    (~line 1492, "...apply_lookahead/apply_learned_bias) ever runs...").
- **`src/game/structs.rs`**:
  - Remove `super::learning::reset_game_log();` from `Game::new()`
    (line 56) and `Game::from_online_parts()` (line 81).
  - Remove `super::learning::finish_game_and_learn(winner);` from
    `Game::game_over()` (line 137).
- **`src/events/mousedown_handler.rs`**:
  - Remove the `if matches!(game.state, GameState::PLAYAI) { ... }` block
    in `end_turn` that calls `record_real_move` (currently ~lines
    384-390), including its explanatory comment.
- **`src/menu.rs`**:
  - Remove `"AI_LEARNING_MODE"` from `SettingsMenu::new`'s checkbox id
    list (line 210).
  - Remove the `"AI_LEARNING_MODE" => set_learning_mode_enabled(flag_value)`
    match arm (lines 239-241).
- **`static/index.html`**:
  - Remove the `checkbox-AI_LEARNING_MODE` `<label class="setting-menu">`
    block (lines 53-60) and the `<p id="learning-progress"></p>` line
    (line 61) from the Settings panel.
- **`js/i18n.js`**:
  - Remove the `settings.aiLearningMode` entry from both the `en` and
    `ja` translation blocks.

## New practice going forward (no code deliverable in this spec)

Extend `ai::perf_tests` one loss at a time, per the workflow above. This
spec's scope is the removal + signature cleanup only -- no new tests are
added as part of implementing this spec itself; they arrive later,
one per reported loss, in future sessions.

## Testing

- After removal, `cargo test`: the entire `game::learning::tests` module
  disappears along with the file. Every remaining test (`ai::perf_tests`
  and everything else in the suite) must still pass unchanged, since this
  is a pure removal + mechanical signature cleanup with no behavior
  change to the surviving heuristic (hard filters, lookahead, scoring).
- No automated test verifies the *absence* of learning -- there's nothing
  meaningful to assert beyond "it compiles and the existing suite is
  green," since removing a nudge that no longer exists isn't independently
  testable behavior.
- Manual verification (browser dev build): open Settings and confirm the
  "AI Learning Mode" checkbox and progress text are gone; play a full
  PLAYAI game on each difficulty and confirm the AI still moves normally
  (using its unchanged hard-filter/lookahead heuristic, no different from
  before this change beyond the removed statistical nudge).

## Explicitly out of scope

- Any new data structure, table, or persistence mechanism for tracking
  AI-improvement history -- the "mechanism" is the test suite itself.
- Migrating or actively deleting previously-persisted IndexedDB data in
  users' browsers.
- Documenting the new workflow anywhere in the repo, by explicit user
  decision.
- Writing any new `perf_tests` entries as part of this change -- those
  come later, one at a time, as real losses get reported in future
  sessions.
