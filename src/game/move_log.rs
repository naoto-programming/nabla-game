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
    fn js_append_move_log_entry(player_num: u32, cards_text: String, changes: String, border_colour: String);
    fn js_clear_move_log();
}

/// call once per match, from Game::new()/Game::from_online_parts, so a new
/// match doesn't show entries left over from a previous one
pub fn reset() {
    js_clear_move_log();
}

/// separates each "slot|before|after" record within the `changes` string
/// passed to js_append_move_log_entry -- a non-printable control character
/// so it can never collide with a card's Display text (see CHANGE_FIELD_SEP)
const CHANGE_RECORD_SEP: char = '\u{1e}';
/// separates the slot/before/after fields within one such record
const CHANGE_FIELD_SEP: char = '\u{1f}';

/// describes a single field slot's content for the log -- an empty string
/// sentinel for a cleared slot (rather than an English word), since the
/// wording ("empty"/"空") is chosen JS-side based on the page's live
/// language (see js/move_log.js) instead of being baked in here
fn describe_slot(field: &Field, index: usize) -> String {
    match &field[index].basis {
        Some(basis) => basis.to_string(),
        None => String::new(),
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
///
/// Sends only language-neutral data (card notation, slot numbers, an empty-
/// string sentinel for a cleared slot) rather than a pre-formatted English
/// sentence -- js/move_log.js assembles the actual display text, choosing
/// wording from the page's live language. Building that here instead would
/// need Rust to know the current language too, and there's no safe way to
/// read js/i18n.js's language state directly: it's a plain JS module
/// imported once by js/index.js, while this file is copied by wasm-bindgen
/// into its own separate snippet instance (see js/online.js's header
/// comment for the same gotcha) -- so a second import of i18n.js from here
/// would get an independent copy of its state, silently stuck on whatever
/// language was active when the page first loaded
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

    let changes = changed_indices
        .iter()
        .map(|&i| {
            format!(
                "{}{}{}{}{}",
                i + 1,
                CHANGE_FIELD_SEP,
                describe_slot(&game.field, i),
                CHANGE_FIELD_SEP,
                describe_slot(new_field, i)
            )
        })
        .collect::<Vec<String>>()
        .join(&CHANGE_RECORD_SEP.to_string());

    let border_colour = unsafe {
        if player_num == 1 { PLAYER_1_COLOUR } else { PLAYER_2_COLOUR }
    };

    js_append_move_log_entry(player_num, cards_text, changes, border_colour.to_string());
}
