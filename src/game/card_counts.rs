// outer crate imports
use super::cards::*;

/// settings-panel names for every distinct card type, in a fixed order matched by
/// `DEFAULT_CARD_COUNTS`/`CARD_COUNTS` and by the HTML input ids ("count-<NAME>")
pub const CARD_COUNT_NAMES: [&str; 21] = [
    "ZERO",
    "ONE",
    "X",
    "X2",
    "COS",
    "SIN",
    "E",
    "DIV",
    "MULT",
    "SQRT",
    "INVERSE",
    "LOG",
    "DERIVATIVE",
    "INTEGRAL",
    "NABLA",
    "LAPLACIAN",
    "LIM_POS_INF",
    "LIM_NEG_INF",
    "LIM_0",
    "LIMINF",
    "LIMSUP",
];

/// original hardcoded deck composition, kept as the reset target. ONE/X/X2 counts
/// include the 2 copies already pre-placed on the starting field (see get_new_deck)
pub const DEFAULT_CARD_COUNTS: [u32; 21] = [
    2, 4, 7, 3, 4, 4, 4, 5, 5, 5, 5, 5, 8, 8, 10, 2, 2, 2, 2, 1, 1,
];

pub static mut CARD_COUNTS: [u32; 21] = DEFAULT_CARD_COUNTS;

/// sets a card count by its settings-panel name (eg. "X2"); ignores unknown names
pub fn set_card_count(name: &str, value: u32) {
    if let Some(index) = CARD_COUNT_NAMES.iter().position(|n| *n == name) {
        unsafe { CARD_COUNTS[index] = value };
    }
}

/// resets every card count back to its original default
pub fn reset_card_counts() {
    unsafe { CARD_COUNTS = DEFAULT_CARD_COUNTS };
}

fn count(name: &str) -> u32 {
    let index = CARD_COUNT_NAMES.iter().position(|n| *n == name).unwrap();
    unsafe { CARD_COUNTS[index] }
}

/// builds a fresh deck using the current card counts
pub fn get_new_deck() -> Vec<Card> {
    let mut deck = vec![];
    deck.extend(vec![Card::BasisCard(BasisCard::Zero); count("ZERO") as usize]);
    // subtract the 2 copies already pre-placed on the starting field
    deck.extend(vec![
        Card::BasisCard(BasisCard::One);
        count("ONE").saturating_sub(2) as usize
    ]);
    deck.extend(vec![
        Card::BasisCard(BasisCard::X);
        count("X").saturating_sub(2) as usize
    ]);
    deck.extend(vec![
        Card::BasisCard(BasisCard::X2);
        count("X2").saturating_sub(2) as usize
    ]);
    deck.extend(vec![Card::BasisCard(BasisCard::Cos); count("COS") as usize]);
    deck.extend(vec![Card::BasisCard(BasisCard::Sin); count("SIN") as usize]);
    deck.extend(vec![Card::BasisCard(BasisCard::E); count("E") as usize]);
    deck.extend(vec![Card::AlgebraicCard(AlgebraicCard::Div); count("DIV") as usize]);
    deck.extend(vec![Card::AlgebraicCard(AlgebraicCard::Mult); count("MULT") as usize]);
    deck.extend(vec![Card::AlgebraicCard(AlgebraicCard::Sqrt); count("SQRT") as usize]);
    deck.extend(vec![
        Card::AlgebraicCard(AlgebraicCard::Inverse);
        count("INVERSE") as usize
    ]);
    deck.extend(vec![Card::AlgebraicCard(AlgebraicCard::Log); count("LOG") as usize]);
    deck.extend(vec![
        Card::DerivativeCard(DerivativeCard::Derivative);
        count("DERIVATIVE") as usize
    ]);
    deck.extend(vec![
        Card::DerivativeCard(DerivativeCard::Integral);
        count("INTEGRAL") as usize
    ]);
    deck.extend(vec![
        Card::DerivativeCard(DerivativeCard::Nabla);
        count("NABLA") as usize
    ]);
    deck.extend(vec![
        Card::DerivativeCard(DerivativeCard::Laplacian);
        count("LAPLACIAN") as usize
    ]);
    deck.extend(vec![
        Card::LimitCard(LimitCard::LimPosInf);
        count("LIM_POS_INF") as usize
    ]);
    deck.extend(vec![
        Card::LimitCard(LimitCard::LimNegInf);
        count("LIM_NEG_INF") as usize
    ]);
    deck.extend(vec![Card::LimitCard(LimitCard::Lim0); count("LIM_0") as usize]);
    deck.extend(vec![Card::LimitCard(LimitCard::Liminf); count("LIMINF") as usize]);
    deck.extend(vec![Card::LimitCard(LimitCard::Limsup); count("LIMSUP") as usize]);
    deck
}
