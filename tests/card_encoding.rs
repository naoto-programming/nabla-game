use nabla_game;
use nabla_game::game::card_encoding::*;
use nabla_game::game::cards::*;

const ALL_CARDS: [Card; 21] = [
    Card::BasisCard(BasisCard::Zero),
    Card::BasisCard(BasisCard::One),
    Card::BasisCard(BasisCard::X),
    Card::BasisCard(BasisCard::X2),
    Card::BasisCard(BasisCard::Cos),
    Card::BasisCard(BasisCard::Sin),
    Card::BasisCard(BasisCard::E),
    Card::AlgebraicCard(AlgebraicCard::Div),
    Card::AlgebraicCard(AlgebraicCard::Mult),
    Card::AlgebraicCard(AlgebraicCard::Sqrt),
    Card::AlgebraicCard(AlgebraicCard::Inverse),
    Card::AlgebraicCard(AlgebraicCard::Log),
    Card::DerivativeCard(DerivativeCard::Derivative),
    Card::DerivativeCard(DerivativeCard::Integral),
    Card::DerivativeCard(DerivativeCard::Nabla),
    Card::DerivativeCard(DerivativeCard::Laplacian),
    Card::LimitCard(LimitCard::LimPosInf),
    Card::LimitCard(LimitCard::LimNegInf),
    Card::LimitCard(LimitCard::Lim0),
    Card::LimitCard(LimitCard::Liminf),
    Card::LimitCard(LimitCard::Limsup),
];

#[test]
fn test_every_card_round_trips_through_a_byte() {
    for card in ALL_CARDS.iter() {
        let byte = card_to_byte(card);
        assert_eq!(byte_to_card(byte), Some(*card), "byte {} did not round-trip", byte);
    }
}

#[test]
fn test_bytes_are_unique() {
    let mut bytes: Vec<u8> = ALL_CARDS.iter().map(card_to_byte).collect();
    bytes.sort();
    bytes.dedup();
    assert_eq!(bytes.len(), ALL_CARDS.len(), "two cards mapped to the same byte");
}

#[test]
fn test_unknown_byte_returns_none() {
    assert_eq!(byte_to_card(255), None);
}

#[test]
fn test_cards_to_bytes_and_back() {
    let hand = vec![
        Card::BasisCard(BasisCard::X),
        Card::DerivativeCard(DerivativeCard::Nabla),
        Card::LimitCard(LimitCard::Limsup),
    ];
    let bytes = cards_to_bytes(&hand);
    assert_eq!(bytes_to_cards(&bytes), Some(hand));
}
