//! Records a PLAYAI match's full move history so it can be exported (via the
//! GAMEOVER "Copy Match Data" button) as one short, copy-pasteable string --
//! meant to be pasted into a bug report so the exact reported match can be
//! reconstructed later, not for any in-app replay/decode feature.
//!
//! The starting deck (captured once, at shuffle time) plus the per-turn click
//! log below are together sufficient to reconstruct the whole match: initial
//! hands and every later redraw are fully determined by popping from that same
//! deck sequence, exactly the way the real game already does (see
//! game/structs.rs::create_players and end_turn's redraw), so nothing besides
//! the deck order and the clicks needs to be recorded.

// outer crate imports
use crate::game::card_encoding::cards_to_bytes;
use crate::game::cards::Card;
use crate::render::util::RenderId;

/// maps every RenderId variant to a stable single byte, and back. Mirrors
/// game/card_encoding.rs's card_to_byte/byte_to_card exactly, for the same
/// reason: keeping the exported string as short as possible
pub(super) fn render_id_to_byte(id: &RenderId) -> u8 {
    match id {
        RenderId::PlayerOne0 => 0,
        RenderId::PlayerOne1 => 1,
        RenderId::PlayerOne2 => 2,
        RenderId::PlayerOne3 => 3,
        RenderId::PlayerOne4 => 4,
        RenderId::PlayerOne5 => 5,
        RenderId::PlayerOne6 => 6,
        RenderId::PlayerTwo0 => 7,
        RenderId::PlayerTwo1 => 8,
        RenderId::PlayerTwo2 => 9,
        RenderId::PlayerTwo3 => 10,
        RenderId::PlayerTwo4 => 11,
        RenderId::PlayerTwo5 => 12,
        RenderId::PlayerTwo6 => 13,
        RenderId::Field0 => 14,
        RenderId::Field1 => 15,
        RenderId::Field2 => 16,
        RenderId::Field3 => 17,
        RenderId::Field4 => 18,
        RenderId::Field5 => 19,
        RenderId::Deck => 20,
        RenderId::Deal => 21,
        RenderId::Graveyard0 => 22,
        RenderId::Graveyard1 => 23,
        RenderId::Graveyard2 => 24,
        RenderId::Cancel => 25,
        RenderId::Multidone => 26,
        RenderId::Confirm => 27,
        RenderId::TurnIndicator => 28,
    }
}

#[cfg(test)]
pub(super) fn byte_to_render_id(byte: u8) -> Option<RenderId> {
    match byte {
        0 => Some(RenderId::PlayerOne0),
        1 => Some(RenderId::PlayerOne1),
        2 => Some(RenderId::PlayerOne2),
        3 => Some(RenderId::PlayerOne3),
        4 => Some(RenderId::PlayerOne4),
        5 => Some(RenderId::PlayerOne5),
        6 => Some(RenderId::PlayerOne6),
        7 => Some(RenderId::PlayerTwo0),
        8 => Some(RenderId::PlayerTwo1),
        9 => Some(RenderId::PlayerTwo2),
        10 => Some(RenderId::PlayerTwo3),
        11 => Some(RenderId::PlayerTwo4),
        12 => Some(RenderId::PlayerTwo5),
        13 => Some(RenderId::PlayerTwo6),
        14 => Some(RenderId::Field0),
        15 => Some(RenderId::Field1),
        16 => Some(RenderId::Field2),
        17 => Some(RenderId::Field3),
        18 => Some(RenderId::Field4),
        19 => Some(RenderId::Field5),
        20 => Some(RenderId::Deck),
        21 => Some(RenderId::Deal),
        22 => Some(RenderId::Graveyard0),
        23 => Some(RenderId::Graveyard1),
        24 => Some(RenderId::Graveyard2),
        25 => Some(RenderId::Cancel),
        26 => Some(RenderId::Multidone),
        27 => Some(RenderId::Confirm),
        28 => Some(RenderId::TurnIndicator),
        _ => None,
    }
}

/// the full shuffled deck as dealt at match start, before create_players splits
/// hands off of it
static mut STARTING_DECK: Vec<Card> = Vec::new();
/// one entry per completed turn (both the human's and the AI's): who moved, and
/// the exact clicks that move consisted of
static mut TURN_LOG: Vec<(u32, Vec<RenderId>)> = Vec::new();
/// clicks buffered so far during the turn currently in progress
static mut CURRENT_TURN_CLICKS: Vec<RenderId> = Vec::new();

/// call once per match, from Game::new(), with the deck immediately after
/// shuffling and before create_players deals from it. Harmless to call for a
/// non-PLAYAI match too (record_click/flush_turn are gated to PLAYAI, so the
/// log this resets just never grows for anything else)
pub fn reset(deck: &[Card]) {
    unsafe {
        STARTING_DECK = deck.to_vec();
        TURN_LOG = Vec::new();
        CURRENT_TURN_CLICKS = Vec::new();
    }
}

/// buffers one click during the current PLAYAI turn -- call from
/// branch_turn_phase (which every click, human or AI, already passes through),
/// gated to GameState::PLAYAI. Mirrors OnlineSession::record_click's exact
/// exclusion rules: Confirm is a pure local commit (nothing new to record),
/// Cancel means nothing was actually committed (discard what's buffered so far)
pub fn record_click(id: RenderId) {
    unsafe {
        match id {
            RenderId::Confirm => {}
            RenderId::Cancel => CURRENT_TURN_CLICKS.clear(),
            _ => CURRENT_TURN_CLICKS.push(id),
        }
    }
}

/// call from end_turn once a move actually commits, with whichever player just
/// moved -- moves the buffered clicks into the permanent turn log
pub fn flush_turn(mover: u32) {
    unsafe {
        let clicks = std::mem::take(&mut CURRENT_TURN_CLICKS);
        if !clicks.is_empty() {
            TURN_LOG.push((mover, clicks));
        }
    }
}

/// serializes the whole recorded match into the most compact byte form:
/// [deck_len:u16][deck bytes] [turn_count:u16] then, per turn,
/// [mover:u8][click_count:u8][click bytes]. u16 for the two lengths that scale
/// with things the player controls (deck size via Settings, match length); u8
/// is enough for mover (always 1 or 2) and click_count (a single move is at
/// most a handful of clicks)
pub(super) fn encode() -> Vec<u8> {
    let (deck, turns) = unsafe { (STARTING_DECK.clone(), TURN_LOG.clone()) };
    let mut bytes = Vec::new();

    let deck_bytes = cards_to_bytes(&deck);
    bytes.extend((deck_bytes.len() as u16).to_le_bytes());
    bytes.extend(deck_bytes);

    bytes.extend((turns.len() as u16).to_le_bytes());
    for (mover, clicks) in &turns {
        bytes.push(*mover as u8);
        bytes.push(clicks.len() as u8);
        bytes.extend(clicks.iter().map(render_id_to_byte));
    }

    bytes
}

/// encodes the current match and copies it to the clipboard as a base64
/// string -- see encode's doc for the byte layout. Uses the browser's native
/// btoa (Window is already an enabled web-sys feature) rather than pulling in
/// a base64 crate; bytes are mapped 1:1 to Latin-1 char codes first, exactly
/// what btoa expects as input
pub fn copy_match_data_to_clipboard() {
    let bytes = encode();
    let binary_string: String = bytes.iter().map(|&b| b as char).collect();
    let base64 = web_sys::window()
        .and_then(|w| w.btoa(&binary_string).ok())
        .unwrap_or_default();
    crate::game::online::copy_to_clipboard(base64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::cards::{AlgebraicCard, BasisCard, DerivativeCard, LimitCard};

    const ALL_RENDER_IDS: [RenderId; 29] = [
        RenderId::PlayerOne0,
        RenderId::PlayerOne1,
        RenderId::PlayerOne2,
        RenderId::PlayerOne3,
        RenderId::PlayerOne4,
        RenderId::PlayerOne5,
        RenderId::PlayerOne6,
        RenderId::PlayerTwo0,
        RenderId::PlayerTwo1,
        RenderId::PlayerTwo2,
        RenderId::PlayerTwo3,
        RenderId::PlayerTwo4,
        RenderId::PlayerTwo5,
        RenderId::PlayerTwo6,
        RenderId::Field0,
        RenderId::Field1,
        RenderId::Field2,
        RenderId::Field3,
        RenderId::Field4,
        RenderId::Field5,
        RenderId::Deck,
        RenderId::Deal,
        RenderId::Graveyard0,
        RenderId::Graveyard1,
        RenderId::Graveyard2,
        RenderId::Cancel,
        RenderId::Multidone,
        RenderId::Confirm,
        RenderId::TurnIndicator,
    ];

    #[test]
    fn test_render_id_byte_round_trip_covers_every_variant() {
        for id in ALL_RENDER_IDS {
            let byte = render_id_to_byte(&id);
            assert_eq!(
                byte_to_render_id(byte),
                Some(id),
                "round trip failed for {id:?} (byte {byte})"
            );
        }
    }

    #[test]
    fn test_render_id_to_byte_assigns_no_duplicate_bytes() {
        let mut bytes: Vec<u8> = ALL_RENDER_IDS.iter().map(render_id_to_byte).collect();
        bytes.sort_unstable();
        bytes.dedup();
        assert_eq!(
            bytes.len(),
            ALL_RENDER_IDS.len(),
            "two RenderId variants share the same byte"
        );
    }

    #[test]
    fn test_record_click_excludes_confirm_and_cancel_clears_buffer() {
        reset(&[]);
        record_click(RenderId::PlayerOne0);
        record_click(RenderId::Field0);
        record_click(RenderId::Confirm); // must not be buffered
        flush_turn(1);

        record_click(RenderId::PlayerTwo0);
        record_click(RenderId::Cancel); // must discard the buffer so far
        record_click(RenderId::PlayerTwo1);
        record_click(RenderId::Field1);
        flush_turn(2);

        let bytes = encode();
        // [deck_len:u16=0][turn_count:u16=2]
        // turn 1: mover=1, click_count=2, [PlayerOne0, Field0]
        // turn 2: mover=2, click_count=2, [PlayerTwo1, Field1] (PlayerTwo0 discarded by Cancel)
        let expected = vec![
            0, 0, // deck_len
            2, 0, // turn_count
            1, 2, render_id_to_byte(&RenderId::PlayerOne0), render_id_to_byte(&RenderId::Field0),
            2, 2, render_id_to_byte(&RenderId::PlayerTwo1), render_id_to_byte(&RenderId::Field1),
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_encode_includes_the_starting_deck() {
        let deck = vec![
            Card::BasisCard(BasisCard::X),
            Card::AlgebraicCard(AlgebraicCard::Mult),
            Card::DerivativeCard(DerivativeCard::Integral),
            Card::LimitCard(LimitCard::Lim0),
        ];
        reset(&deck);
        let bytes = encode();

        let deck_len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        assert_eq!(deck_len, deck.len());
        let deck_bytes = &bytes[2..2 + deck_len];
        assert_eq!(deck_bytes.to_vec(), cards_to_bytes(&deck));

        // no turns recorded -- turn_count follows immediately after the deck
        let turn_count = u16::from_le_bytes([bytes[2 + deck_len], bytes[3 + deck_len]]);
        assert_eq!(turn_count, 0);
        assert_eq!(bytes.len(), 2 + deck_len + 2);
    }
}
