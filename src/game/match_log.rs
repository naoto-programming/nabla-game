//! Records a PLAYAI match's full move history so it can be exported (via the
//! GAMEOVER "Copy Match Data" button) as one copy-pasteable string -- meant to
//! be pasted into a bug report so the exact reported match can be understood
//! directly from the string, without needing to replay it through the game
//! engine first (an earlier version of this recorded only raw clicks, which
//! meant reconstructing what actually happened required manually re-deriving
//! the resulting field's symbolic math by hand). Not for any in-app
//! replay/decode feature.
//!
//! Every completed turn records the mover, their hand as it was when they
//! decided (so it's clear what their options actually were), which card(s)
//! they played, and the resulting field right after -- everything needed to
//! read a match turn-by-turn directly off the decoded string.

// outer crate imports
use crate::game::card_encoding::cards_to_bytes;
use crate::game::cards::Card;
use crate::game::field::Field;

/// one completed turn: who moved, their hand before this move (what their
/// options were), which card(s) they played (2 for a Mult/Div play combining
/// a field slot with a hand card, 1 otherwise), and the field immediately
/// after the move
struct TurnRecord {
    mover: u32,
    hand_before: Vec<Card>,
    cards_played: Vec<Card>,
    resulting_field: Field,
}

/// the full shuffled deck as dealt at match start, before create_players splits
/// hands off of it -- not otherwise used by the log (every turn already carries
/// its own hand_before snapshot), but kept so the very first turn's hand can be
/// cross-checked against where the match actually started
static mut STARTING_DECK: Vec<Card> = Vec::new();
/// one entry per completed turn (both the human's and the AI's)
static mut TURN_LOG: Vec<TurnRecord> = Vec::new();

/// call once per match, from Game::new(), with the deck immediately after
/// shuffling and before create_players deals from it. Harmless to call for a
/// non-PLAYAI match too (record_turn is gated to PLAYAI, so the log this
/// resets just never grows for anything else)
pub fn reset(deck: &[Card]) {
    unsafe {
        STARTING_DECK = deck.to_vec();
        TURN_LOG = Vec::new();
    }
}

/// call from end_turn once a move commits, before the played cards are
/// actually removed from the hand -- see its call site for why that ordering
/// matters (hand_before must reflect the hand as the player actually saw it)
pub fn record_turn(mover: u32, hand_before: &[Card], cards_played: Vec<Card>, resulting_field: &Field) {
    unsafe {
        TURN_LOG.push(TurnRecord {
            mover,
            hand_before: hand_before.to_vec(),
            cards_played,
            resulting_field: resulting_field.clone(),
        });
    }
}

/// serializes one field's 6 slots into their Display strings (empty string for
/// an empty slot) -- readable directly off the decoded bytes, not a structural
/// encoding of the Basis tree, since this log is for a human/Claude to read,
/// not for the app itself to parse back
fn field_slot_strings(field: &Field) -> [String; 6] {
    std::array::from_fn(|i| field[i].basis.as_ref().map(|b| b.to_string()).unwrap_or_default())
}

/// appends a length-prefixed UTF-8 string (u16 length, since a deeply nested
/// expression's Display string can run well past 255 bytes)
fn push_string(bytes: &mut Vec<u8>, s: &str) {
    let s_bytes = s.as_bytes();
    bytes.extend((s_bytes.len() as u16).to_le_bytes());
    bytes.extend(s_bytes);
}

/// appends a length-prefixed card list (u8 count -- a hand never exceeds 7,
/// cards_played never exceeds 2)
fn push_cards(bytes: &mut Vec<u8>, cards: &[Card]) {
    bytes.push(cards.len() as u8);
    bytes.extend(cards_to_bytes(cards));
}

/// serializes the whole recorded match: [deck_len:u16][deck bytes], then
/// [turn_count:u16] and, per turn, [mover:u8] [hand_before] [cards_played]
/// [6 resulting-field-slot strings, in field order 0..6]. u16 for the lengths
/// that scale with things the player controls (deck size via Settings, match
/// length, and field-expression length for a long game's nested expressions)
pub(super) fn encode() -> Vec<u8> {
    let (deck, turns) = unsafe { (STARTING_DECK.clone(), &TURN_LOG) };
    let mut bytes = Vec::new();

    let deck_bytes = cards_to_bytes(&deck);
    bytes.extend((deck_bytes.len() as u16).to_le_bytes());
    bytes.extend(deck_bytes);

    bytes.extend((turns.len() as u16).to_le_bytes());
    for turn in turns {
        bytes.push(turn.mover as u8);
        push_cards(&mut bytes, &turn.hand_before);
        push_cards(&mut bytes, &turn.cards_played);
        for slot in field_slot_strings(&turn.resulting_field) {
            push_string(&mut bytes, &slot);
        }
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
    use crate::basis::structs::Basis;
    use crate::game::cards::{AlgebraicCard, BasisCard, DerivativeCard, LimitCard};
    use crate::game::field::FieldBasis;
    use std::sync::Mutex;

    /// these tests share STARTING_DECK/TURN_LOG (both `static mut`), and Rust's
    /// default test runner executes tests in parallel threads -- without this,
    /// one test's reset()/record_turn() calls race another's, corrupting both
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn sample_field() -> Field {
        let mut field = Field::new();
        field[0] = FieldBasis::new(&Basis::from(BasisCard::Cos));
        field[1] = FieldBasis::none();
        field
    }

    /// helper mirroring encode()'s own u16-length-prefixed string format, for
    /// tests to build an expected byte sequence without duplicating encode()'s
    /// internals
    fn prefixed_string(s: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_string(&mut bytes, s);
        bytes
    }

    #[test]
    fn test_encode_includes_the_starting_deck() {
        let _guard = TEST_LOCK.lock().unwrap();
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

    #[test]
    fn test_encode_includes_hand_cards_played_and_resulting_field() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset(&[]);
        let hand_before = vec![
            Card::DerivativeCard(DerivativeCard::Integral),
            Card::BasisCard(BasisCard::Cos),
        ];
        let cards_played = vec![Card::BasisCard(BasisCard::Cos)];
        let field = sample_field();
        record_turn(2, &hand_before, cards_played.clone(), &field);

        let bytes = encode();
        // [deck_len:u16=0][turn_count:u16=1]
        let mut expected = vec![0, 0, 1, 0];
        expected.push(2); // mover
        expected.push(2); // hand_before count
        expected.extend(cards_to_bytes(&hand_before));
        expected.push(1); // cards_played count
        expected.extend(cards_to_bytes(&cards_played));
        for slot in field_slot_strings(&field) {
            expected.extend(prefixed_string(&slot));
        }
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_field_slot_strings_reads_cos_and_empty_correctly() {
        let field = sample_field();
        let slots = field_slot_strings(&field);
        assert_eq!(slots[0], Basis::from(BasisCard::Cos).to_string());
        assert_eq!(slots[1], "");
    }

    #[test]
    fn test_two_turns_encode_and_decode_boundaries_correctly() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset(&[]);
        record_turn(1, &[Card::BasisCard(BasisCard::X)], vec![Card::BasisCard(BasisCard::X)], &Field::new());
        record_turn(
            2,
            &[Card::BasisCard(BasisCard::Cos), Card::BasisCard(BasisCard::Zero)],
            vec![Card::BasisCard(BasisCard::Cos), Card::BasisCard(BasisCard::Zero)],
            &sample_field(),
        );

        let bytes = encode();
        let turn_count = u16::from_le_bytes([bytes[2], bytes[3]]);
        assert_eq!(turn_count, 2);

        // manually walk the byte stream to confirm the two turns don't overlap
        // or corrupt each other's boundaries
        let mut pos = 4usize;
        for expected_mover in [1u8, 2u8] {
            assert_eq!(bytes[pos], expected_mover);
            pos += 1;
            let hand_count = bytes[pos] as usize;
            pos += 1 + hand_count;
            let played_count = bytes[pos] as usize;
            pos += 1 + played_count;
            for _ in 0..6 {
                let str_len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
                pos += 2 + str_len;
            }
        }
        assert_eq!(pos, bytes.len(), "byte walk didn't land exactly on the end of the buffer");
    }
}
