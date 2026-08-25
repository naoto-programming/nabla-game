// std imports
use std::fmt::{Display, Formatter, Result};
// external crate imports
use gloo::timers::callback::Timeout;
use rand::Rng;
// outer crate imports
use crate::basis::structs::*;
use crate::events::mousedown_handler::{branch_turn_phase, next_turn};
use crate::game::cards::*;
use crate::game::field::{Field, FieldBasis};
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

/// one candidate move: the click sequence that plays it, its immediate heuristic
/// score, and the field it would leave behind (used to look one ply ahead at the
/// opponent's best reply -- see apply_lookahead)
struct AiMove {
    clicks: Vec<RenderId>,
    score: f64,
    resulting_field: Field,
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
    let game = unsafe { GAME.as_ref().unwrap() };
    let candidates =
        generate_candidates_for(AI_PLAYER_NUM, &game.player_2, &game.field, game.turn.number);
    if candidates.is_empty() {
        // no legal move within the set the AI can evaluate (eg. hand is entirely
        // basis cards with no empty field slot to play them into). A human in this
        // spot would just be stuck with no way to end their turn either, but leaving
        // the AI stuck here would silently stall the whole game (turn never advances,
        // and nothing then stops a human from clicking through it as player 2) --
        // so the AI forfeits the turn instead of hanging it
        next_turn();
        return;
    }

    let difficulty = unsafe { AI_DIFFICULTY };
    let chosen = choose_move(candidates, difficulty, game.turn.number);
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

/// which player's colour field slot `target` renders in (see draw() in render.rs) --
/// slots 0-2 are player 2's, 3-5 are player 1's
fn field_owner(target: usize) -> u32 {
    if target < 3 {
        2
    } else {
        1
    }
}

/// scores replacing `field[target]` with `new_basis`, from `evaluating_player`'s
/// point of view: rewards clearing/simplifying the OTHER player's slot (progress
/// toward winning), penalises clearing evaluating_player's own slot. Reused both to
/// score the AI's own candidates (evaluating_player = AI_PLAYER_NUM) and to predict
/// the human's best reply one ply ahead (evaluating_player = 1) -- see apply_lookahead
fn score_replacement(evaluating_player: u32, target: usize, new_basis: &Basis, field: &Field) -> f64 {
    let is_opponent_side = field_owner(target) != evaluating_player;
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

/// scores a Nabla/Laplacian play across all 3 slots of the targeted half, from
/// `evaluating_player`'s point of view (see score_replacement)
fn score_half(evaluating_player: u32, half_start: usize, is_laplacian: bool, field: &Field) -> f64 {
    (half_start..half_start + 3)
        .filter_map(|i| field[i].basis.as_ref().map(|basis| (i, basis)))
        .map(|(i, basis)| {
            let once = derivative(basis);
            let result = if is_laplacian { derivative(&once) } else { once };
            score_replacement(evaluating_player, i, &result, field)
        })
        .sum()
}

/// applies a Nabla/Laplacian half-field derivative to a cloned field, mirroring
/// handle_derivative_card's non-history-shortcut path. Used only to build a
/// candidate's resulting_field for lookahead purposes -- a reasonable approximation
/// for ranking candidates, since the AI's actual chosen move is always executed
/// afterward through the real branch_turn_phase pipeline, which handles the history
/// shortcut correctly regardless of what this function computed
fn apply_half_to_field(half_start: usize, is_laplacian: bool, field: &Field) -> Field {
    let mut new_field = field.clone();
    for i in half_start..half_start + 3 {
        if let Some(basis) = field[i].basis.clone() {
            let once = derivative(&basis);
            let result = if is_laplacian { derivative(&once) } else { once };
            new_field[i] = if result.is_num(0) {
                FieldBasis::none()
            } else {
                FieldBasis::new(&result)
            };
        }
    }
    new_field
}

/// enumerates legal moves `player_num` knows how to evaluate from `hand` against
/// `field`, generalized so the same logic can score the AI's own candidates and
/// predict the opponent's best reply one ply ahead (see apply_lookahead). Mult/Div
/// are restricted to pairs of field bases (skipping hand-basis operands and 3+-way
/// combinations) to keep the search small; every other card type is fully covered
fn generate_candidates_for(
    player_num: u32,
    hand: &[Card],
    field: &Field,
    turn_number: u32,
) -> Vec<AiMove> {
    let mut moves = vec![];

    for (i, card) in hand.iter().enumerate() {
        let hand_id = RenderId::from(format!("p{player_num}={i}"));

        match card {
            Card::BasisCard(basis_card) if !matches!(basis_card, BasisCard::Zero) => {
                for target in 0..6 {
                    if field[target].basis.is_none() {
                        let new_basis = Basis::from(*basis_card);
                        let mut resulting_field = field.clone();
                        resulting_field[target] = FieldBasis::new(&new_basis);
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={target}"))],
                            score: score_replacement(player_num, target, &new_basis, field),
                            resulting_field,
                        });
                    }
                }
            }
            Card::DerivativeCard(DerivativeCard::Nabla) => {
                for half_start in [0usize, 3usize] {
                    moves.push(AiMove {
                        clicks: vec![hand_id, RenderId::from(format!("f={half_start}"))],
                        score: score_half(player_num, half_start, false, field),
                        resulting_field: apply_half_to_field(half_start, false, field),
                    });
                }
            }
            Card::DerivativeCard(DerivativeCard::Laplacian) if turn_number >= 2 => {
                for half_start in [0usize, 3usize] {
                    moves.push(AiMove {
                        clicks: vec![hand_id, RenderId::from(format!("f={half_start}"))],
                        score: score_half(player_num, half_start, true, field),
                        resulting_field: apply_half_to_field(half_start, true, field),
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
                        let score = score_replacement(player_num, a, &result, field)
                            + score_replacement(player_num, b, &Basis::from(0), field)
                            - 1.0; // drop the double-counted baseline from scoring twice
                        let mut resulting_field = field.clone();
                        resulting_field[b] = FieldBasis::none();
                        resulting_field[a] = if result.is_num(0) {
                            FieldBasis::none()
                        } else {
                            FieldBasis::new(&result)
                        };
                        moves.push(AiMove {
                            clicks: vec![
                                hand_id,
                                RenderId::from(format!("f={a}")),
                                RenderId::from(format!("f={b}")),
                                RenderId::Multidone,
                            ],
                            score,
                            resulting_field,
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
                        let mut resulting_field = field.clone();
                        resulting_field[target] = if result.is_num(0) {
                            FieldBasis::none()
                        } else {
                            FieldBasis::new(&result)
                        };
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={target}"))],
                            score: score_replacement(player_num, target, &result, field),
                            resulting_field,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    moves
}

/// the single best immediate score `player_num` could achieve against `field` with
/// `hand` -- used to estimate how dangerous a position is for whoever's left
/// occupying it (ie. the opponent's best reply after one of the AI's candidate
/// moves). 0.0 (neutral) if they'd have no legal move to evaluate at all, since being
/// stuck isn't scored as a loss by this heuristic (see take_ai_turn's own forfeit
/// handling for the AI's symmetric case)
fn best_reply_score(player_num: u32, hand: &[Card], field: &Field, turn_number: u32) -> f64 {
    generate_candidates_for(player_num, hand, field, turn_number)
        .iter()
        .map(|mv| mv.score)
        .fold(f64::MIN, f64::max)
        .max(0.0)
}

/// how many of the AI's top-scored candidates get the (more expensive) opponent
/// lookahead applied -- bounds the O(pool * opponent_candidates) search so a turn
/// with many candidates (eg. several Mult/Div cards in hand) still resolves quickly.
/// Candidates outside this pool were already meaningfully worse by immediate score,
/// so skipping their lookahead is an acceptable approximation
const LOOKAHEAD_POOL: usize = 15;
/// how heavily the opponent's best reply weighs against the AI's own immediate gain
/// when ranking moves -- tuned so a move that hands the opponent a big reply is
/// usually worse than a smaller move that doesn't, without completely overriding a
/// large enough immediate gain
const LOOKAHEAD_WEIGHT: f64 = 0.75;

/// re-ranks the AI's best candidates by subtracting a weighted estimate of the
/// human's best reply one ply ahead, so the AI stops walking into moves that look
/// good immediately but hand the opponent an even better follow-up (eg. clearing an
/// opponent slot while leaving one of its own exposed to an easy kill next turn --
/// the original one-ply-only heuristic couldn't see this at all)
fn apply_lookahead(mut candidates: Vec<AiMove>, turn_number: u32) -> Vec<AiMove> {
    let game = unsafe { GAME.as_ref().unwrap() };
    let opponent_hand = &game.player_1;

    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    for mv in candidates.iter_mut().take(LOOKAHEAD_POOL) {
        let reply = best_reply_score(1, opponent_hand, &mv.resulting_field, turn_number + 1);
        mv.score -= LOOKAHEAD_WEIGHT * reply;
    }
    candidates
}

/// difficulty-scaled selection: Hard and Medium both look one ply ahead (see
/// apply_lookahead) before ranking -- Hard always takes the best-adjusted move
/// (strong enough that walking into a bad trade is rare), Medium picks randomly from
/// a shrinking top slice of the adjusted ranking. Easy skips lookahead entirely and
/// stays mostly random, so it remains reliably beatable
fn choose_move(candidates: Vec<AiMove>, difficulty: AiDifficulty, turn_number: u32) -> AiMove {
    let mut candidates = match difficulty {
        AiDifficulty::Hard | AiDifficulty::Medium => apply_lookahead(candidates, turn_number),
        AiDifficulty::Easy => candidates,
    };
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
