// std imports
use std::fmt::{Display, Formatter, Result};
// external crate imports
use gloo::timers::callback::Timeout;
use rand::Rng;
// outer crate imports
use crate::basis::structs::*;
use crate::events::mousedown_handler::branch_turn_phase;
use crate::game::cards::*;
use crate::game::field::Field;
use crate::game::structs::*;
use crate::math::derivative::derivative;
use crate::render::util::RenderId;
// root imports
use crate::GAME;

/// the AI always plays as player 2; player 1 (who goes first) is always human
pub const AI_PLAYER_NUM: u32 = 2;

pub static mut AI_DIFFICULTY: AiDifficulty = AiDifficulty::Medium;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiDifficulty {
    Easy,
    Medium,
    Hard,
}
impl From<&str> for AiDifficulty {
    fn from(s: &str) -> Self {
        match s {
            "EASY" => AiDifficulty::Easy,
            "HARD" => AiDifficulty::Hard,
            _ => AiDifficulty::Medium,
        }
    }
}
impl Display for AiDifficulty {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "{}",
            match self {
                AiDifficulty::Easy => "EASY",
                AiDifficulty::Medium => "MEDIUM",
                AiDifficulty::Hard => "HARD",
            }
        )
    }
}

/// one candidate move: the click sequence that plays it, and its heuristic score
struct AiMove {
    clicks: Vec<RenderId>,
    score: f64,
}

/// if it's now the AI's turn in a PLAYAI game, schedules its move after a short
/// delay (purely for pacing -- an instant move reads as unresponsive/broken)
pub fn maybe_take_ai_turn() {
    let game = unsafe { GAME.as_ref().unwrap() };
    if !matches!(game.state, GameState::PLAYAI) || game.get_current_player_num() != AI_PLAYER_NUM {
        return;
    }
    Timeout::new(700, take_ai_turn).forget();
}

fn take_ai_turn() {
    let candidates = generate_candidates();
    if candidates.is_empty() {
        // no legal move within the set the AI can evaluate (eg. hand is all Mult/Div
        // with no fitting field pair); nothing to do this turn
        return;
    }

    let difficulty = unsafe { AI_DIFFICULTY };
    let chosen = choose_move(candidates, difficulty);
    for id in chosen.clicks {
        // call the turn-phase logic directly (bypassing handle_mousedown) so the AI's
        // clicks don't also toggle the human-facing card-overflow expand/collapse state
        branch_turn_phase(id, AI_PLAYER_NUM);
    }

    // if CONFIRM_BEFORE_PLAY is on, the AI's own move now awaits confirmation --
    // there's no human to review it, so confirm immediately
    let game = unsafe { GAME.as_ref().unwrap() };
    if matches!(game.turn.phase, TurnPhase::CONFIRM) {
        branch_turn_phase(RenderId::Confirm, AI_PLAYER_NUM);
    }
}

/// counts nodes in a Basis tree, used as a cheap complexity/"simplicity" proxy
fn basis_size(basis: &Basis) -> u32 {
    match basis {
        Basis::BasisLeaf(_) => 1,
        Basis::BasisNode(node) => 1 + node.operands.iter().map(basis_size).sum::<u32>(),
    }
}

/// scores replacing `field[target]` with `new_basis`: rewards clearing/simplifying an
/// opponent slot (progress toward winning), penalises clearing the AI's own slot
fn score_replacement(target: usize, new_basis: &Basis, field: &Field) -> f64 {
    // slots 0-2 render in player 2's colour and losing them empty is what makes
    // player 2 lose (see next_turn's win check) -- ie. 0-2 is the AI's OWN side,
    // and 3-5 (rendered in player 1's colour) is the human opponent's side
    let is_opponent_side = target >= 3;
    let old_size = field[target].basis.as_ref().map(basis_size).unwrap_or(0);
    let new_size = if new_basis.is_num(0) {
        0
    } else {
        basis_size(new_basis)
    };

    let mut score = 1.0; // baseline: any legal move beats none
    if is_opponent_side {
        score += if new_size == 0 && old_size > 0 {
            100.0 // cleared an opponent slot entirely
        } else {
            (old_size as f64 - new_size as f64) * 5.0
        };
    } else {
        score += if new_size == 0 && old_size > 0 {
            -50.0 // avoid clearing our own slot
        } else {
            (old_size as f64 - new_size as f64) * 1.0
        };
    }
    score
}

/// scores a Nabla/Laplacian play across all 3 slots of the targeted half
fn score_half(half_start: usize, is_laplacian: bool, field: &Field) -> f64 {
    (half_start..half_start + 3)
        .filter_map(|i| field[i].basis.as_ref().map(|basis| (i, basis)))
        .map(|(i, basis)| {
            let once = derivative(basis);
            let result = if is_laplacian { derivative(&once) } else { once };
            score_replacement(i, &result, field)
        })
        .sum()
}

/// enumerates legal moves the AI knows how to evaluate. Mult/Div are restricted to
/// pairs of field bases (skipping hand-basis operands and 3+-way combinations) to
/// keep the search small; every other card type is fully covered
fn generate_candidates() -> Vec<AiMove> {
    let game = unsafe { GAME.as_ref().unwrap() };
    let hand = &game.player_2;
    let field = &game.field;
    let mut moves = vec![];

    for (i, card) in hand.iter().enumerate() {
        let hand_id = RenderId::from(format!("p2={}", i));

        match card {
            Card::BasisCard(basis_card) if !matches!(basis_card, BasisCard::Zero) => {
                for target in 0..6 {
                    if field[target].basis.is_none() {
                        let new_basis = Basis::from(*basis_card);
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={}", target))],
                            score: score_replacement(target, &new_basis, field),
                        });
                    }
                }
            }
            Card::DerivativeCard(DerivativeCard::Nabla) => {
                for half_start in [0usize, 3usize] {
                    moves.push(AiMove {
                        clicks: vec![hand_id, RenderId::from(format!("f={}", half_start))],
                        score: score_half(half_start, false, field),
                    });
                }
            }
            Card::DerivativeCard(DerivativeCard::Laplacian) if game.turn.number >= 2 => {
                for half_start in [0usize, 3usize] {
                    moves.push(AiMove {
                        clicks: vec![hand_id, RenderId::from(format!("f={}", half_start))],
                        score: score_half(half_start, true, field),
                    });
                }
            }
            Card::AlgebraicCard(AlgebraicCard::Div | AlgebraicCard::Mult) => {
                for a in 0..6 {
                    for b in 0..6 {
                        if a == b || field[a].basis.is_none() || field[b].basis.is_none() {
                            continue;
                        }
                        let bases = vec![
                            field[a].basis.as_ref().unwrap().clone(),
                            field[b].basis.as_ref().unwrap().clone(),
                        ];
                        let result = apply_multi_card(card, bases);
                        // both operand slots get cleared first; the result (if
                        // non-zero) then goes back into `a` -- `b` is always lost, so
                        // it must be scored too, or the AI can't see that it's
                        // sacrificing (say) its own slot to simplify an opponent's
                        let score = score_replacement(a, &result, field)
                            + score_replacement(b, &Basis::from(0), field)
                            - 1.0; // drop the double-counted baseline from scoring twice
                        moves.push(AiMove {
                            clicks: vec![
                                hand_id,
                                RenderId::from(format!("f={}", a)),
                                RenderId::from(format!("f={}", b)),
                                RenderId::Multidone,
                            ],
                            score,
                        });
                    }
                }
            }
            // everything else that reaches SELECT phase as a single-target operator:
            // Derivative, Integral, Inverse, Log, Sqrt, and all LimitCard variants
            Card::DerivativeCard(DerivativeCard::Derivative | DerivativeCard::Integral)
            | Card::AlgebraicCard(AlgebraicCard::Inverse | AlgebraicCard::Log | AlgebraicCard::Sqrt)
            | Card::LimitCard(_) => {
                for target in 0..6 {
                    if let Some(basis) = &field[target].basis {
                        let result = apply_card(card)(basis);
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={}", target))],
                            score: score_replacement(target, &result, field),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    moves
}

/// difficulty-scaled selection: Hard always takes the best-scored move, Medium picks
/// randomly from a shrinking top slice, Easy is mostly random with occasional sense
fn choose_move(mut candidates: Vec<AiMove>, difficulty: AiDifficulty) -> AiMove {
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    let mut rng = rand::thread_rng();

    let index = match difficulty {
        AiDifficulty::Hard => 0,
        AiDifficulty::Medium => {
            let pool = ((candidates.len() as f64) * 0.34).ceil().max(1.0) as usize;
            rng.gen_range(0..pool.min(candidates.len()))
        }
        AiDifficulty::Easy => {
            if rng.gen_bool(0.25) {
                let pool = candidates.len().min(3);
                rng.gen_range(0..pool)
            } else {
                rng.gen_range(0..candidates.len())
            }
        }
    };
    candidates.remove(index)
}
