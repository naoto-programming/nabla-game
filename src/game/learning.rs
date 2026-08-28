//! Self-play reinforcement learning: replaces the old real-time minimax search
//! as the source of the AI's strength (see ai::choose_move's doc). Instead of
//! spending seconds searching every real decision, the AI plays fast heuristic
//! moves nudged by a small table of (board shape, move shape) -> win rate
//! statistics accumulated from self-play games and from real human-vs-AI games.
//!
//! State/action canonicalization deliberately stays coarse (a handful of bits
//! per slot, not the exact symbolic expression) -- the raw expression space is
//! unbounded, so learning at that granularity would almost never see the same
//! state twice. Coarse shape buckets mean unrelated games teach each other
//! something, at the cost of not distinguishing eg. "x^2" from "x^3".

// std imports
use std::collections::HashMap;
use std::convert::TryInto;
// external crate imports
use gloo::timers::callback::Interval;
use rand::seq::SliceRandom;
use wasm_bindgen::prelude::*;
// outer crate imports
use crate::basis::structs::*;
use crate::game::ai::{
    self, choose_move, field_owner, generate_candidates_for, opponent_of, side_is_cleared,
    AiDifficulty, AiMove,
};
use crate::game::card_counts::get_new_deck;
use crate::game::cards::*;
use crate::game::field::Field;
use crate::game::structs::create_players;

/* ---------------------------------------------------------------------------
 * State/action canonicalization
 * ------------------------------------------------------------------------- */

/// coarse classification of a single field slot's top-level shape, ignoring
/// coefficients and nested structure -- eg. "3cos(x)" and "-cos(2x)" are both
/// just Trig. Based on Basis's top-level BasisLeaf element / BasisNode operator
/// (see basis/structs.rs)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotShape {
    Empty,
    Constant,
    Linear,
    Polynomial,
    Trig,
    Exponential,
    Log,
    Other,
}

fn classify_shape(basis: Option<&Basis>) -> SlotShape {
    match basis {
        None => SlotShape::Empty,
        Some(Basis::BasisLeaf(leaf)) => match leaf.element {
            BasisElement::Num => SlotShape::Constant,
            BasisElement::X => SlotShape::Linear,
            BasisElement::Inf => SlotShape::Other,
        },
        Some(Basis::BasisNode(node)) => match node.operator {
            BasisOperator::Sin | BasisOperator::Cos | BasisOperator::Asin | BasisOperator::Acos => {
                SlotShape::Trig
            }
            BasisOperator::E => SlotShape::Exponential,
            BasisOperator::Log => SlotShape::Log,
            BasisOperator::Pow(_) => SlotShape::Polynomial,
            BasisOperator::Add
            | BasisOperator::Minus
            | BasisOperator::Mult
            | BasisOperator::Div
            | BasisOperator::Inv
            | BasisOperator::Int => SlotShape::Other,
        },
    }
}

/// coarse node-count bucket (0 = empty, 4 = "4 or more") -- see ai::basis_size
fn size_bucket(basis: Option<&Basis>) -> u64 {
    basis.map(ai::basis_size).unwrap_or(0).min(4) as u64
}

/// packs one slot's shape (3 bits) + size bucket (3 bits) into 6 bits
fn encode_slot(basis: Option<&Basis>) -> u64 {
    ((classify_shape(basis) as u64) << 3) | size_bucket(basis)
}

/// opening/midgame/endgame bucket -- the same board shape can call for different
/// priorities depending on how much of the deck is likely still left
fn phase_bucket(turn_number: u32) -> u64 {
    if turn_number < 5 {
        0
    } else if turn_number < 15 {
        1
    } else {
        2
    }
}

/// whether `hand` contains a Mult/Div card or a Zero card -- the single
/// highest-impact synergy called out by evaluate_game_situation/
/// zero_creation_bonus (see ai.rs): holding either changes whether clearing a
/// slot now vs. setting one up is the better plan, which a pure board snapshot
/// can't tell on its own
fn hand_has_synergy(hand: &[Card]) -> bool {
    hand.iter().any(|c| {
        matches!(
            c,
            Card::AlgebraicCard(AlgebraicCard::Mult | AlgebraicCard::Div)
                | Card::BasisCard(BasisCard::Zero)
        )
    })
}

/// canonicalized board snapshot from `mover`'s point of view: mover's 3 slots
/// then the opponent's 3 slots (always in that order, regardless of which
/// player number is actually moving, so mirror-image games share history), plus
/// phase and hand-synergy bits. 6*6 + 2 + 1 = 39 bits
fn encode_state(field: &Field, mover: u32, turn_number: u32, hand: &[Card]) -> u64 {
    let mut key = 0u64;
    for i in 0..6 {
        if field_owner(i) == mover {
            key = (key << 6) | encode_slot(field[i].basis.as_ref());
        }
    }
    for i in 0..6 {
        if field_owner(i) != mover {
            key = (key << 6) | encode_slot(field[i].basis.as_ref());
        }
    }
    key = (key << 2) | phase_bucket(turn_number);
    key = (key << 1) | (hand_has_synergy(hand) as u64);
    key
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SizeDelta {
    Shrank,
    Same,
    Grew,
}

fn delta_bucket(old_total: u32, new_total: u32) -> SizeDelta {
    if new_total < old_total {
        SizeDelta::Shrank
    } else if new_total > old_total {
        SizeDelta::Grew
    } else {
        SizeDelta::Same
    }
}

/// canonicalizes what a move DID to the board, purely from the before/after
/// field snapshots -- deliberately independent of which card caused it, so
/// mult/div, derivative, and basis-placement moves that produce the same shape
/// transition share learned history instead of fragmenting it by card identity.
/// own/opponent slots cleared (2 bits each), own/opponent total-size delta
/// direction (2 bits each), and the pre-move shape of whichever slot changed
/// the most (3 bits) -- 11 bits total
fn encode_action(field: &Field, resulting_field: &Field, mover: u32) -> u64 {
    let mut own_cleared = 0u64;
    let mut opp_cleared = 0u64;
    let mut own_old_total = 0u32;
    let mut own_new_total = 0u32;
    let mut opp_old_total = 0u32;
    let mut opp_new_total = 0u32;
    let mut primary_target = 0usize;
    let mut biggest_change: i64 = -1;

    for i in 0..6 {
        let old_size = field[i].basis.as_ref().map(ai::basis_size).unwrap_or(0);
        let new_size = resulting_field[i]
            .basis
            .as_ref()
            .map(ai::basis_size)
            .unwrap_or(0);

        if field_owner(i) == mover {
            own_old_total += old_size;
            own_new_total += new_size;
            if old_size > 0 && new_size == 0 {
                own_cleared += 1;
            }
        } else {
            opp_old_total += old_size;
            opp_new_total += new_size;
            if old_size > 0 && new_size == 0 {
                opp_cleared += 1;
            }
        }

        let change = (old_size as i64 - new_size as i64).abs();
        if change > biggest_change {
            biggest_change = change;
            primary_target = i;
        }
    }

    let shape_before = classify_shape(field[primary_target].basis.as_ref());
    let own_delta = delta_bucket(own_old_total, own_new_total);
    let opp_delta = delta_bucket(opp_old_total, opp_new_total);

    let mut key = own_cleared.min(3);
    key = (key << 2) | opp_cleared.min(3);
    key = (key << 2) | (own_delta as u64);
    key = (key << 2) | (opp_delta as u64);
    key = (key << 3) | (shape_before as u64);
    key
}

/// combined (state, action) lookup key -- 39 state bits + 11 action bits fits
/// comfortably in a u64, so the whole learned table is just a HashMap<u64, _>
fn learning_key(field: &Field, resulting_field: &Field, mover: u32, turn_number: u32, hand: &[Card]) -> u64 {
    (encode_state(field, mover, turn_number, hand) << 11) | encode_action(field, resulting_field, mover)
}

/* ---------------------------------------------------------------------------
 * Real-game recording: every move of an in-progress PLAYAI match (the human's
 * and the AI's alike), so the learned table improves from real play too, not
 * just self-play -- see record_real_move/finish_game_and_learn's callers in
 * events/mousedown_handler.rs and game/structs.rs
 * ------------------------------------------------------------------------- */

/// Vec::new() is a const fn (unlike HashMap::new(), which needs a random seed
/// at runtime) so this can be initialized directly, no Option/lazy-init needed
static mut CURRENT_GAME_MOVES: Vec<(u32, u64)> = Vec::new();

/// clears the in-progress move log -- call whenever a fresh Game begins (see
/// Game::new/from_online_parts), so a restarted match never inherits moves
/// left over from whatever game came before it
pub fn reset_game_log() {
    unsafe {
        CURRENT_GAME_MOVES.clear();
    }
}

/// records one real move (human or AI) from an in-progress PLAYAI match. Safe
/// to call for any game mode -- callers gate this on GameState::PLAYAI, but an
/// empty/irrelevant log for other modes is harmless since finish_game_and_learn
/// only ever credits whatever ended up recorded
pub fn record_real_move(field: &Field, resulting_field: &Field, mover: u32, turn_number: u32, hand: &[Card]) {
    let key = learning_key(field, resulting_field, mover, turn_number, hand);
    unsafe {
        CURRENT_GAME_MOVES.push((mover, key));
    }
}

/// drains the in-progress move log into the learned table now that `winner` is
/// known, crediting every recorded move the same way record_game_outcome does
/// for a self-play game. A no-op if nothing was recorded (eg. a PLAYVS/
/// PLAYONLINE match, where record_real_move is never called)
pub fn finish_game_and_learn(winner: u32) {
    let moves = unsafe { std::mem::take(&mut CURRENT_GAME_MOVES) };
    if moves.is_empty() {
        return;
    }
    record_game_outcome(&moves, winner);
}

/* ---------------------------------------------------------------------------
 * Learned table (Monte Carlo control: visits/wins per (state, action) key)
 * ------------------------------------------------------------------------- */

/// u16 (not u32): visits/wins only ever inform a win-rate ratio (see
/// apply_learned_bias), so saturating at 65535 costs no meaningful precision
/// while halving the on-disk size of every persisted record -- see
/// serialize_table, which relies on this being exactly 4 bytes
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LearnedEntry {
    pub(super) visits: u16,
    pub(super) wins: u16,
}

/// single-threaded by construction (WASM's main thread) -- matches this
/// codebase's existing `static mut` globals (see GAME in lib.rs, AI_DIFFICULTY
/// in ai.rs) rather than pulling in a synchronization primitive nothing here
/// actually needs
static mut LEARNED_TABLE: Option<HashMap<u64, LearnedEntry>> = None;

fn table() -> &'static mut HashMap<u64, LearnedEntry> {
    unsafe {
        if LEARNED_TABLE.is_none() {
            LEARNED_TABLE = Some(HashMap::new());
        }
        LEARNED_TABLE.as_mut().unwrap()
    }
}

/// number of distinct (state, action) patterns learned so far -- surfaced to
/// the settings UI as a progress indicator
pub fn learned_pattern_count() -> usize {
    table().len()
}

/// replaces the whole learned table (used when loading persisted data on
/// startup -- see learning.js/on_learned_data_loaded)
pub(super) fn load_table(entries: HashMap<u64, LearnedEntry>) {
    unsafe {
        LEARNED_TABLE = Some(entries);
    }
}

/// records one game's outcome into the learned table: every recorded (mover,
/// key) pair gets `visits` incremented, and `wins` incremented too if that
/// particular move was made by the side that ended up winning. Called once per
/// finished game (self-play or real), not per move, so a single loss doesn't
/// erase everything learned from a move that was actually fine
pub(super) fn record_game_outcome(moves: &[(u32, u64)], winner: u32) {
    let table = table();
    for &(mover, key) in moves {
        let entry = table.entry(key).or_default();
        entry.visits = entry.visits.saturating_add(1);
        if mover == winner {
            entry.wins = entry.wins.saturating_add(1);
        }
    }
}

const MIN_CONFIDENT_VISITS: u16 = 8;
const BLEND_WEIGHT: f64 = 40.0;

/// nudges each candidate's score by what history says about moves that made a
/// similar shape transition from a similar board shape (see learning_key).
/// Ignored below MIN_CONFIDENT_VISITS -- a handful of games says more about
/// luck (who drew what) than about the move itself, so an unproven pattern
/// shouldn't move the needle yet. The blend is small relative to
/// score_replacement's few-hundred-point swings for a clean win/loss-relevant
/// play: this is meant to break ties among moves the hand-tuned heuristic
/// already considers roughly equal, not override its judgement
pub(super) fn apply_learned_bias(
    mut candidates: Vec<AiMove>,
    hand: &[Card],
    field: &Field,
    mover: u32,
    turn_number: u32,
) -> Vec<AiMove> {
    let table = table();
    for mv in candidates.iter_mut() {
        let key = learning_key(field, &mv.resulting_field, mover, turn_number, hand);
        if let Some(entry) = table.get(&key) {
            if entry.visits >= MIN_CONFIDENT_VISITS {
                let win_rate = entry.wins as f64 / entry.visits as f64;
                mv.score += (win_rate - 0.5) * BLEND_WEIGHT;
            }
        }
    }
    candidates
}

/* ---------------------------------------------------------------------------
 * Headless self-play (training data source, no DOM/GAME dependency)
 * ------------------------------------------------------------------------- */

/// generous but finite cap on turns per simulated game -- a pure safety valve
/// against a pathological cycle (eg. both sides only ever making purely
/// defensive plays against each other) hanging the background training loop,
/// not a tuned gameplay parameter. Real games settle in well under this
const SELF_PLAY_TURN_LIMIT: u32 = 300;

/// plays one full game entirely in memory, using `choose_move` for both sides
/// (each can be a different difficulty, so eg. a Hard AI can train against a
/// weaker one), and returns the winner plus every (mover, learning key) pair
/// recorded along the way so the caller can credit them via
/// record_game_outcome once the winner is known. Returns None for a game that
/// hit SELF_PLAY_TURN_LIMIT without a winner -- the caller should simply
/// discard that game's moves rather than credit a fabricated result.
///
/// Deliberately reimplements just the rules that affect who wins (redraw up to
/// 7 cards at end of turn, same win condition as side_is_cleared) rather than
/// reusing end_turn/next_turn in events/mousedown_handler.rs, which are wired
/// directly to the live GAME/MENU singletons and browser rendering -- this runs
/// during both `cargo test` and the browser's background training loop, neither
/// of which should touch (or need) a real in-progress match
pub fn simulate_self_play_game(difficulty_1: AiDifficulty, difficulty_2: AiDifficulty) -> Option<(u32, Vec<(u32, u64)>)> {
    let mut deck = get_new_deck();
    deck.shuffle(&mut rand::thread_rng());
    let (mut hand_1, mut hand_2) = create_players(&mut deck);
    let mut field = Field::new();
    let mut turn_number = 0u32;
    let mut recorded_moves = vec![];

    loop {
        if turn_number > SELF_PLAY_TURN_LIMIT {
            return None;
        }

        let mover = if turn_number % 2 == 0 { 1 } else { 2 };
        let difficulty = if mover == 1 { difficulty_1 } else { difficulty_2 };
        let opponent_hand = if mover == 1 { hand_2.clone() } else { hand_1.clone() };
        let mover_hand = if mover == 1 { &mut hand_1 } else { &mut hand_2 };

        let candidates = generate_candidates_for(mover, mover_hand, &field, turn_number);
        if candidates.is_empty() {
            // stuck -- forfeit the turn exactly like the real AI does (see
            // try_take_ai_turn's own empty-candidates handling)
            turn_number += 1;
            continue;
        }

        let chosen = choose_move(
            candidates,
            mover,
            mover_hand,
            &opponent_hand,
            difficulty,
            turn_number,
            &field,
        );
        let key = learning_key(&field, &chosen.resulting_field, mover, turn_number, mover_hand);
        recorded_moves.push((mover, key));

        // remove consumed cards highest-index-first so removing one doesn't
        // shift the position of another still to be removed (same ordering
        // end_turn uses for its own selected_indices)
        let mut consumed = chosen.consumed_hand_indices.clone();
        consumed.sort_unstable();
        consumed.reverse();
        for idx in consumed {
            mover_hand.remove(idx);
        }
        field = chosen.resulting_field;

        if side_is_cleared(&field, opponent_of(mover)) {
            return Some((mover, recorded_moves));
        }

        let cards_to_deal = 7usize.saturating_sub(mover_hand.len()).min(deck.len());
        for _ in 0..cards_to_deal {
            if let Some(card) = deck.pop() {
                mover_hand.push(card);
            }
        }

        turn_number += 1;
    }
}

/// plays `count` self-play games and folds every decisive one into the learned
/// table -- the unit of work the background training loop (and the initial
/// pre-seeding pass) repeats. Alternates which side gets Hard/Medium/Easy across
/// games so the table isn't only ever trained from one matchup. Returns how
/// many of the `count` games were decisive (informational only)
pub fn run_self_play_batch(count: u32) -> u32 {
    const DIFFICULTIES: [AiDifficulty; 3] = [AiDifficulty::Easy, AiDifficulty::Medium, AiDifficulty::Hard];
    let mut decisive = 0;
    for i in 0..count {
        let difficulty_1 = DIFFICULTIES[(i as usize) % DIFFICULTIES.len()];
        let difficulty_2 = DIFFICULTIES[((i as usize) / DIFFICULTIES.len()) % DIFFICULTIES.len()];
        if let Some((winner, moves)) = simulate_self_play_game(difficulty_1, difficulty_2) {
            record_game_outcome(&moves, winner);
            decisive += 1;
        }
    }
    decisive
}

/* ---------------------------------------------------------------------------
 * Persistence (IndexedDB via js/learning.js) -- callback-based like
 * game/online.rs, not async/await: wasm-bindgen-futures is only a
 * dev-dependency here, and every other browser-API integration in this
 * codebase already follows the fire-and-be-called-back-later shape
 * ------------------------------------------------------------------------- */

#[wasm_bindgen(module = "/js/learning.js")]
extern "C" {
    #[wasm_bindgen(js_name = js_load_learned_table)]
    fn js_load_learned_table();
    #[wasm_bindgen(js_name = js_save_learned_table)]
    fn js_save_learned_table(bytes: Vec<u8>);
}

/// fixed-size 12-byte record per pattern (8-byte key + 2-byte visits + 2-byte
/// wins, all little-endian), back to back with no framing -- every record is
/// the same size, so the byte length alone is enough to parse it back
fn serialize_table(table: &HashMap<u64, LearnedEntry>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(table.len() * 12);
    for (&key, entry) in table.iter() {
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(&entry.visits.to_le_bytes());
        bytes.extend_from_slice(&entry.wins.to_le_bytes());
    }
    bytes
}

fn deserialize_table(bytes: &[u8]) -> HashMap<u64, LearnedEntry> {
    let mut table = HashMap::new();
    for chunk in bytes.chunks_exact(12) {
        let key = u64::from_le_bytes(chunk[0..8].try_into().unwrap());
        let visits = u16::from_le_bytes(chunk[8..10].try_into().unwrap());
        let wins = u16::from_le_bytes(chunk[10..12].try_into().unwrap());
        table.insert(key, LearnedEntry { visits, wins });
    }
    table
}

/// kicks off loading persisted learning data -- call once at startup (see
/// lib.rs). Asynchronous: the table stays empty until on_learned_data_loaded
/// fires back from JS (same load-then-callback shape as online play's
/// on_init_received), so a page that starts playing immediately briefly plays
/// against an empty table rather than blocking startup on IndexedDB
pub fn load_persisted_table() {
    js_load_learned_table();
}

#[wasm_bindgen]
pub fn on_learned_data_loaded(bytes: Vec<u8>) {
    if !bytes.is_empty() {
        load_table(deserialize_table(&bytes));
    }
}

/// writes the current table out to IndexedDB -- called periodically by the
/// self-play loop (see start_self_play_loop) and once more when learning mode
/// is turned off, rather than after every single move/game: an interrupted
/// browser tab loses at most one save interval's worth of progress, which is
/// an acceptable trade for not hitting IndexedDB constantly
pub fn save_persisted_table() {
    js_save_learned_table(serialize_table(table()));
}

/* ---------------------------------------------------------------------------
 * Background self-play loop ("AI学習モード" in Settings)
 * ------------------------------------------------------------------------- */

pub static mut LEARNING_MODE_ENABLED: bool = false;
static mut SELF_PLAY_INTERVAL_HANDLE: Option<Interval> = None;
static mut GAMES_PLAYED_THIS_SESSION: u32 = 0;

// deliberately conservative: this competes with the main thread for CPU time
// while the tab is open (including during a real game, since simulate_self_play_game
// has no dependency on GAME and so is safe to run concurrently with one) --
// a small bounded amount of work per tick keeps any single tick cheap even if
// a self-play game lands on a slow/deeply-nested worst case, rather than
// risking a visible stutter for the sake of faster training throughput
const SELF_PLAY_BATCH_SIZE: u32 = 3;
const SELF_PLAY_TICK_MS: u32 = 150;
const SAVE_EVERY_N_TICKS: u32 = 40; // roughly every 6s at the tick rate above
static mut TICKS_SINCE_SAVE: u32 = 0;

/// toggled from the Settings checkbox (see menu.rs) -- starts or stops the
/// background training loop and flushes to IndexedDB on either transition, so
/// turning learning off doesn't strand whatever was learned since the last
/// periodic save
pub fn set_learning_mode_enabled(enabled: bool) {
    unsafe {
        LEARNING_MODE_ENABLED = enabled;
    }
    if enabled {
        start_self_play_loop();
    } else {
        stop_self_play_loop();
        save_persisted_table();
    }
    update_progress_display();
}

fn start_self_play_loop() {
    unsafe {
        if SELF_PLAY_INTERVAL_HANDLE.is_some() {
            return; // already running
        }
        TICKS_SINCE_SAVE = 0;
        let interval = Interval::new(SELF_PLAY_TICK_MS, self_play_tick);
        SELF_PLAY_INTERVAL_HANDLE = Some(interval);
    }
}

/// dropping a gloo Interval cancels it, so clearing the handle is the whole
/// stop operation -- no separate "cancel" call needed
fn stop_self_play_loop() {
    unsafe {
        SELF_PLAY_INTERVAL_HANDLE = None;
    }
}

fn self_play_tick() {
    let decisive = run_self_play_batch(SELF_PLAY_BATCH_SIZE);
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

/// reflects training progress into the Settings panel -- see
/// #learning-progress in index.html. Best-effort: silently does nothing if the
/// element isn't there (eg. this fires from a stray tick right as the page is
/// tearing down)
fn update_progress_display() {
    let enabled = unsafe { LEARNING_MODE_ENABLED };
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(document) => document,
        None => return,
    };
    let element = match document.get_element_by_id("learning-progress") {
        Some(element) => element,
        None => return,
    };
    if !enabled {
        element.set_text_content(Some(""));
        return;
    }
    let games = unsafe { GAMES_PLAYED_THIS_SESSION };
    element.set_text_content(Some(&format!(
        "{} patterns learned / {} self-play games this session",
        learned_pattern_count(),
        games
    )));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::field::FieldBasis;

    #[test]
    fn test_encode_slot_distinguishes_shapes() {
        let empty = encode_slot(None);
        let constant = encode_slot(Some(&Basis::from(1)));
        let linear = encode_slot(Some(&Basis::x()));
        let cos = encode_slot(Some(&Basis::from(BasisCard::Cos)));
        assert_ne!(empty, constant);
        assert_ne!(constant, linear);
        assert_ne!(linear, cos);
    }

    #[test]
    fn test_encode_state_is_symmetric_from_either_players_perspective() {
        // a field that looks identical from player 1's seat as it does from
        // player 2's seat (mirrored) must encode to the same state key --
        // otherwise the table would never generalize between mirror-image games
        let mut field = Field::new();
        for i in 0..6 {
            field[i] = FieldBasis::none();
        }
        field[0] = FieldBasis::new(&Basis::x());
        field[3] = FieldBasis::new(&Basis::x());

        let hand: Vec<Card> = vec![];
        let state_as_player_1 = encode_state(&field, 1, 3, &hand);
        let state_as_player_2 = encode_state(&field, 2, 3, &hand);
        assert_eq!(state_as_player_1, state_as_player_2);
    }

    #[test]
    fn test_encode_action_flags_a_full_clear_of_opponent_slot() {
        // slot 3 belongs to player 1 (see field_owner) -- from mover 2's point of
        // view, that's the opponent's slot
        let mut field = Field::new();
        field[3] = FieldBasis::new(&Basis::x());
        let mut resulting = field.clone();
        resulting[3] = FieldBasis::none();

        // opp_cleared occupies bits 7..=8 of the 11-bit action key (own_cleared
        // is the top 2 bits, opp_cleared the next 2)
        let action = encode_action(&field, &resulting, 2);
        let opp_cleared = (action >> 7) & 0b11;
        assert_eq!(opp_cleared, 1);
    }

    #[test]
    fn test_record_and_lookup_round_trip() {
        load_table(HashMap::new());
        let key = 12345u64;
        record_game_outcome(&[(1, key), (1, key), (2, key)], 1);
        let table = table();
        let entry = table.get(&key).unwrap();
        assert_eq!(entry.visits, 3);
        assert_eq!(entry.wins, 2);
    }

    #[test]
    fn test_apply_learned_bias_rewards_a_historically_winning_pattern_and_punishes_a_losing_one() {
        load_table(HashMap::new());

        let mut field = Field::new();
        for i in 0..6 {
            field[i] = FieldBasis::none();
        }
        // deliberately different shapes (Linear vs Trig) so clearing one vs. the
        // other is actually distinguishable -- two slots with the *same* shape
        // are legitimately indistinguishable under this coarse encoding (see
        // test_encode_action's own doc), so that wouldn't test anything here
        field[0] = FieldBasis::new(&Basis::x());
        field[1] = FieldBasis::new(&Basis::from(BasisCard::Cos));
        let hand: Vec<Card> = vec![];
        let turn_number = 3;

        let mut winning_result = field.clone();
        winning_result[0] = FieldBasis::none();
        let mut losing_result = field.clone();
        losing_result[1] = FieldBasis::none();

        let winning_key = learning_key(&field, &winning_result, 2, turn_number, &hand);
        let losing_key = learning_key(&field, &losing_result, 2, turn_number, &hand);
        // distinct actions from the same state (clearing the Linear slot vs. the
        // Trig slot) must not collide -- otherwise the blend below would cancel
        // itself out
        assert_ne!(winning_key, losing_key);

        record_game_outcome(&vec![(2, winning_key); MIN_CONFIDENT_VISITS as usize], 2);
        record_game_outcome(&vec![(2, losing_key); MIN_CONFIDENT_VISITS as usize], 1);

        let candidates = vec![
            AiMove {
                clicks: vec![],
                score: 0.0,
                resulting_field: winning_result,
                wins_immediately: false,
                hurts_self_or_helps_opponent: false,
                consumed_hand_indices: vec![],
            },
            AiMove {
                clicks: vec![],
                score: 0.0,
                resulting_field: losing_result,
                wins_immediately: false,
                hurts_self_or_helps_opponent: false,
                consumed_hand_indices: vec![],
            },
        ];

        let biased = apply_learned_bias(candidates, &hand, &field, 2, turn_number);
        assert!(biased[0].score > 0.0, "a pattern that always won should get a positive nudge");
        assert!(biased[1].score < 0.0, "a pattern that always lost should get a negative nudge");
    }

    #[test]
    fn test_apply_learned_bias_ignores_unproven_patterns() {
        load_table(HashMap::new());

        let mut field = Field::new();
        for i in 0..6 {
            field[i] = FieldBasis::none();
        }
        field[0] = FieldBasis::new(&Basis::x());
        let hand: Vec<Card> = vec![];
        let turn_number = 3;

        let mut result = field.clone();
        result[0] = FieldBasis::none();
        let key = learning_key(&field, &result, 2, turn_number, &hand);
        // only one visit -- well under MIN_CONFIDENT_VISITS
        record_game_outcome(&[(2, key)], 2);

        let candidates = vec![AiMove {
            clicks: vec![],
            score: 0.0,
            resulting_field: result,
            wins_immediately: false,
            hurts_self_or_helps_opponent: false,
            consumed_hand_indices: vec![],
        }];
        let biased = apply_learned_bias(candidates, &hand, &field, 2, turn_number);
        assert_eq!(biased[0].score, 0.0);
    }

    /// end-to-end sanity check: a full headless self-play game must terminate
    /// (not hang or panic) and produce either a clean winner or an explicit
    /// "no decision reached" result -- never anything else
    #[test]
    fn test_self_play_game_terminates_with_a_winner_or_no_result() {
        for _ in 0..20 {
            match simulate_self_play_game(AiDifficulty::Medium, AiDifficulty::Medium) {
                None => {}
                Some((winner, moves)) => {
                    assert!(winner == 1 || winner == 2);
                    assert!(!moves.is_empty());
                }
            }
        }
    }

    #[test]
    fn test_self_play_batch_grows_the_learned_table() {
        load_table(HashMap::new());
        assert_eq!(learned_pattern_count(), 0);
        run_self_play_batch(10);
        assert!(learned_pattern_count() > 0, "10 self-play games should teach at least one pattern");
    }
}
