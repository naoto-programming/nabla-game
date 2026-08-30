//! Live, in-game log of which hand card(s) were played on which field
//! slot(s), shown in a DOM panel (see js/move_log.js) and gated behind the
//! SHOW_MOVE_LOG setting. Unlike match_log (PLAYAI-only, meant for exporting a
//! full match for bug reports), this applies to every 2-player GameState and
//! shows only what's visible going forward -- no history is kept while the
//! setting is off, and turning it on mid-match doesn't back-fill past turns.

// wasm-bindgen imports
use wasm_bindgen::prelude::*;
// outer crate imports
use crate::game::cards::Card;
use crate::game::field::Field;
use crate::game::flags::SHOW_MOVE_LOG;
use crate::game::structs::Game;
use crate::render::util::{PLAYER_1_COLOUR, PLAYER_2_COLOUR};

#[wasm_bindgen(module = "/js/move_log.js")]
extern "C" {
    fn js_append_move_log_entry(text: String, border_colour: String);
    fn js_clear_move_log();
}

/// call once per match, from Game::new()/Game::from_online_parts, so a new
/// match doesn't show entries left over from a previous one
pub fn reset() {
    js_clear_move_log();
}

/// describes a single field slot's content for the log ("empty" for a
/// cleared slot rather than an empty string, which would read as a typo)
fn describe_slot(field: &Field, index: usize) -> String {
    match &field[index].basis {
        Some(basis) => basis.to_string(),
        None => "empty".to_string(),
    }
}

/// call right before a move's resulting field is actually committed (ie.
/// where `game.field = new_field` happens, in both the immediate-commit and
/// CONFIRM_BEFORE_PLAY-deferred paths), passing the caller's own `game`
/// reference rather than re-fetching the GAME static independently -- the
/// caller already holds `&mut Game` at that point, and taking a second,
/// separate reference to the same `static mut` while that one is still live
/// is a soundness trap (technically UB, and a real miscompilation risk under
/// release-mode optimization, which is what the deployed build actually
/// uses) even though the values read would otherwise be correct. `game.field`
/// must still be the pre-move field when this is called, and
/// `game.active.selected` must still hold this move's clicks (same
/// invariant end_turn's own cards_played computation relies on)
pub fn record_move(game: &Game, new_field: &Field, changed_indices: &[usize]) {
    if !unsafe { SHOW_MOVE_LOG } {
        return;
    }

    let player_num = game.get_current_player_num();
    let hand = if player_num == 1 { &game.player_1 } else { &game.player_2 };
    let cards_played: Vec<Card> = game
        .active
        .selected
        .iter()
        .filter(|id| id.is_player())
        .map(|id| hand[id.key_val().1])
        .collect();

    let cards_text = cards_played
        .iter()
        .map(|card| card.to_string())
        .collect::<Vec<String>>()
        .join(" + ");

    let slots_text = changed_indices
        .iter()
        .map(|&i| format!("slot {}: {} \u{2192} {}", i + 1, describe_slot(&game.field, i), describe_slot(new_field, i)))
        .collect::<Vec<String>>()
        .join(", ");

    let border_colour = unsafe {
        if player_num == 1 { PLAYER_1_COLOUR } else { PLAYER_2_COLOUR }
    };

    js_append_move_log_entry(
        format!("P{}: {} on {}", player_num, cards_text, slots_text),
        border_colour.to_string(),
    );
}
