// outer crate imports
use super::cards::*;

/// maps every Card variant to a stable single byte, and back. Used only for the
/// one-time transfer of the shuffled deck + starting hands when an online match
/// begins (see game/online.rs) -- every move after that is relayed as clicks,
/// not cards, so this never needs to handle anything but the 21 card kinds.
pub fn card_to_byte(card: &Card) -> u8 {
    match card {
        Card::BasisCard(BasisCard::Zero) => 0,
        Card::BasisCard(BasisCard::One) => 1,
        Card::BasisCard(BasisCard::X) => 2,
        Card::BasisCard(BasisCard::X2) => 3,
        Card::BasisCard(BasisCard::Cos) => 4,
        Card::BasisCard(BasisCard::Sin) => 5,
        Card::BasisCard(BasisCard::E) => 6,
        Card::AlgebraicCard(AlgebraicCard::Div) => 7,
        Card::AlgebraicCard(AlgebraicCard::Mult) => 8,
        Card::AlgebraicCard(AlgebraicCard::Sqrt) => 9,
        Card::AlgebraicCard(AlgebraicCard::Inverse) => 10,
        Card::AlgebraicCard(AlgebraicCard::Log) => 11,
        Card::DerivativeCard(DerivativeCard::Derivative) => 12,
        Card::DerivativeCard(DerivativeCard::Integral) => 13,
        Card::DerivativeCard(DerivativeCard::Nabla) => 14,
        Card::DerivativeCard(DerivativeCard::Laplacian) => 15,
        Card::LimitCard(LimitCard::LimPosInf) => 16,
        Card::LimitCard(LimitCard::LimNegInf) => 17,
        Card::LimitCard(LimitCard::Lim0) => 18,
        Card::LimitCard(LimitCard::Liminf) => 19,
        Card::LimitCard(LimitCard::Limsup) => 20,
    }
}

pub fn byte_to_card(byte: u8) -> Option<Card> {
    match byte {
        0 => Some(Card::BasisCard(BasisCard::Zero)),
        1 => Some(Card::BasisCard(BasisCard::One)),
        2 => Some(Card::BasisCard(BasisCard::X)),
        3 => Some(Card::BasisCard(BasisCard::X2)),
        4 => Some(Card::BasisCard(BasisCard::Cos)),
        5 => Some(Card::BasisCard(BasisCard::Sin)),
        6 => Some(Card::BasisCard(BasisCard::E)),
        7 => Some(Card::AlgebraicCard(AlgebraicCard::Div)),
        8 => Some(Card::AlgebraicCard(AlgebraicCard::Mult)),
        9 => Some(Card::AlgebraicCard(AlgebraicCard::Sqrt)),
        10 => Some(Card::AlgebraicCard(AlgebraicCard::Inverse)),
        11 => Some(Card::AlgebraicCard(AlgebraicCard::Log)),
        12 => Some(Card::DerivativeCard(DerivativeCard::Derivative)),
        13 => Some(Card::DerivativeCard(DerivativeCard::Integral)),
        14 => Some(Card::DerivativeCard(DerivativeCard::Nabla)),
        15 => Some(Card::DerivativeCard(DerivativeCard::Laplacian)),
        16 => Some(Card::LimitCard(LimitCard::LimPosInf)),
        17 => Some(Card::LimitCard(LimitCard::LimNegInf)),
        18 => Some(Card::LimitCard(LimitCard::Lim0)),
        19 => Some(Card::LimitCard(LimitCard::Liminf)),
        20 => Some(Card::LimitCard(LimitCard::Limsup)),
        _ => None,
    }
}

pub fn cards_to_bytes(cards: &[Card]) -> Vec<u8> {
    cards.iter().map(card_to_byte).collect()
}

/// returns None if any byte is unrecognised (eg. corrupted/foreign message)
pub fn bytes_to_cards(bytes: &[u8]) -> Option<Vec<Card>> {
    bytes.iter().map(|b| byte_to_card(*b)).collect()
}
