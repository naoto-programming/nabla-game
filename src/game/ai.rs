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

/// flag to prevent AI from taking multiple turns in a single real turn
pub static mut AI_IS_TAKING_TURN: bool = false;

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
/// score, the field it would leave behind (used to look one ply ahead at the
/// opponent's best reply -- see apply_lookahead), and whether it wins outright
struct AiMove {
    clicks: Vec<RenderId>,
    score: f64,
    resulting_field: Field,
    /// true if this move empties every slot belonging to the *other* player --
    /// the actual game-over condition (see side_is_cleared) -- ie. this move wins
    /// the game immediately, regardless of what the heuristic score says
    wins_immediately: bool,
    /// true if this move shrinks any of the mover's own field slots, or grows any
    /// of the opponent's -- see hurts_self_or_helps_opponent
    hurts_self_or_helps_opponent: bool,
}

fn opponent_of(player_num: u32) -> u32 {
    if player_num == 1 {
        2
    } else {
        1
    }
}

/// the 3 field slots belonging to `owner` (see field_owner)
fn owned_slots(owner: u32) -> [usize; 3] {
    if owner == 2 {
        [0, 1, 2]
    } else {
        [3, 4, 5]
    }
}

/// true if every slot belonging to `owner` is empty -- the actual win/loss
/// condition (see next_turn in events/mousedown_handler.rs): whoever's own side
/// empties out loses, the other player wins
fn side_is_cleared(field: &Field, owner: u32) -> bool {
    owned_slots(owner).iter().all(|&i| field[i].basis.is_none())
}

/// true if `player_num` has ANY legal move that would win immediately against
/// `field` with `hand`. Deliberately exhaustive (always searches Mult/Div, unlike
/// best_reply_score's cheap danger estimate) -- this feeds the highest-priority
/// checks (an immediate win, or an immediate loss to avoid), where missing a
/// winning Mult/Div combination would be a real correctness bug, not just an
/// acceptable approximation
fn has_winning_move(player_num: u32, hand: &[Card], field: &Field, turn_number: u32) -> bool {
    generate_candidates_for(player_num, hand, field, turn_number)
        .iter()
        .any(|mv| mv.wins_immediately)
}

/// if it's now the AI's turn in a PLAYAI game, schedules its move after a short
/// delay (purely for pacing -- an instant move reads as unresponsive/broken)
pub fn maybe_take_ai_turn() {
    let game = unsafe { GAME.as_ref().unwrap() };
    if !matches!(game.state, GameState::PLAYAI) || game.get_current_player_num() != AI_PLAYER_NUM {
        return;
    }
    // Prevent AI from taking multiple turns in a single real turn
    if unsafe { AI_IS_TAKING_TURN } {
        return;
    }
    unsafe { AI_IS_TAKING_TURN = true };
    Timeout::new(700, take_ai_turn).forget();
}

/// runs the AI's actual decision + click execution -- split out from take_ai_turn
/// purely so the latter can wrap this call in catch_unwind (see its doc)
fn try_take_ai_turn() {
    let game = unsafe { GAME.as_ref().unwrap() };
    let candidates = generate_candidates_for(
        AI_PLAYER_NUM,
        &game.player_2,
        &game.field,
        game.turn.number,
    );
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
    let chosen = choose_move(
        candidates,
        &game.player_2,
        &game.player_1,
        difficulty,
        game.turn.number,
        &game.field,
        DEFAULT_MINIMAX_TIME_BUDGET,
    );
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

/// entry point scheduled by maybe_take_ai_turn. Wraps try_take_ai_turn in
/// catch_unwind as a last-resort safety net: an unexpected panic anywhere in move
/// generation/selection/execution (eg. an edge case in the symbolic math that
/// wasn't caught in testing) would otherwise leave AI_IS_TAKING_TURN stuck true
/// forever -- silently disabling the AI for the rest of the match, since
/// maybe_take_ai_turn would then bail out on every future call without ever
/// explaining why. Always resets the flag and forfeits the turn instead, the same
/// graceful fallback already used when the AI has no legal move at all
fn take_ai_turn() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(try_take_ai_turn));
    unsafe { AI_IS_TAKING_TURN = false };
    if let Err(payload) = result {
        // console_error_panic_hook (enabled unconditionally, see lib.rs) already
        // printed the real panic message + source location the moment it
        // happened; this just makes the forfeit-instead-of-freeze outcome
        // unambiguous right next to it, including the message again in case that
        // line scrolled by unnoticed
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(non-string panic payload)".to_string());
        web_sys::console::error_1(
            &format!("AI move panicked ({message}) -- forfeiting its turn instead of freezing").into(),
        );
        next_turn();
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

/// true if playing `player_num`'s move (which turns `field` into `resulting_field`)
/// ever clears one of `player_num`'s own slots entirely, or grows one of the
/// opponent's -- "attacking your own field" specifically means erasing one of its
/// cards (a slot going from occupied to empty), not merely simplifying it while it
/// stays occupied: the actual loss condition only cares whether a slot is empty or
/// not, so shrinking your own x^2 down to x is not a step toward losing the way
/// clearing it to nothing is, and blocking it as if it were needlessly cost the AI
/// perfectly good simplifying/defensive plays on its own side. Checks every field
/// slot, so this catches every move type uniformly -- single-target operators,
/// Nabla/Laplacian's half-field derivative, Mult/Div's two-slot combine (both the
/// result slot and the sacrificed one), and BasisCard placement into an empty slot
/// (size 0 -> positive counts as "growing" that slot, so filling the opponent's
/// empty slot is caught here too) -- without needing a special case wired into
/// each one individually
fn hurts_self_or_helps_opponent(player_num: u32, field: &Field, resulting_field: &Field) -> bool {
    (0..6).any(|i| {
        let old_size = field[i].basis.as_ref().map(basis_size).unwrap_or(0);
        let new_size = resulting_field[i].basis.as_ref().map(basis_size).unwrap_or(0);
        if field_owner(i) == player_num {
            new_size == 0 && old_size > 0
        } else {
            new_size > old_size
        }
    })
}

/// scores replacing `field[target]` with `new_basis`, from `evaluating_player`'s
/// point of view: rewards clearing/simplifying the OTHER player's slot (progress
/// toward winning), penalises clearing evaluating_player's own slot. Reused both to
/// score the AI's own candidates (evaluating_player = AI_PLAYER_NUM) and to predict
/// the human's best reply one ply ahead (evaluating_player = 1) -- see apply_lookahead.
/// Deliberately doesn't factor in how many slots remain occupied overall -- see
/// strategic_slot_bonus, added by each caller separately (once per candidate, not
/// once per changed slot; see its own doc for why that split matters)
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
            200.0 // cleared an opponent slot entirely - highest priority
        } else if new_size < old_size {
            (old_size as f64 - new_size as f64) * 10.0 // strongly reward simplifying opponent
        } else if new_size > old_size {
            (old_size as f64 - new_size as f64) * 20.0 // heavily penalize strengthening opponent
        } else {
            0.0 // neutral change
        };
    } else {
        score += if new_size == 0 && old_size > 0 {
            -200.0 // heavily penalize clearing our own slot
        } else if new_size < old_size {
            (old_size as f64 - new_size as f64) * 5.0 // penalize simplifying own slot
        } else if new_size > old_size {
            (old_size as f64 - new_size as f64) * 2.0 // mildly reward strengthening own slot
        } else {
            0.0 // neutral change
        };
    }
    score
}

/// bonus/penalty for how many slots remain occupied on each side in
/// `resulting_field`, from `evaluating_player`'s point of view: fewer occupied
/// opponent slots is rewarded (closer to winning), fewer occupied own slots is
/// penalised more heavily (closer to losing). Takes the true resulting field
/// directly rather than reconstructing a hypothetical one from a single changed
/// target -- Mult/Div changes TWO slots in one move (the merged result and the
/// sacrificed slot), and computing this per-target from score_replacement (as an
/// earlier version did) meant calling it once per changed slot, each blind to the
/// other slot's simultaneous change -- eg. scoring the kept slot's call as if the
/// sacrificed slot were still occupied, and vice versa -- systematically
/// mis-valuing exactly the moves this bonus exists to rank (own-side Mult/Div
/// consolidation moves) whenever the mover's own side wasn't already fully
/// occupied. Called exactly once per candidate, regardless of how many slots it
/// changes, so it can't double-count
fn strategic_slot_bonus(evaluating_player: u32, resulting_field: &Field) -> f64 {
    let opponent_slots_remaining = (0..6)
        .filter(|&i| field_owner(i) != evaluating_player && resulting_field[i].basis.is_some())
        .count() as f64;
    let own_slots_remaining = (0..6)
        .filter(|&i| field_owner(i) == evaluating_player && resulting_field[i].basis.is_some())
        .count() as f64;
    (3.0 - opponent_slots_remaining) * 15.0 - (3.0 - own_slots_remaining) * 25.0
}

/// scores a Nabla/Laplacian play across all 3 slots of the targeted half, from
/// `evaluating_player`'s point of view (see score_replacement). Whether this ends
/// up favouring or hurting `evaluating_player` is left entirely to score_replacement
/// -- applying it to their own half isn't hard-excluded here since a derivative can
/// occasionally *grow* an expression (eg. product rule), which the general
/// hurts_self_or_helps_opponent filter (based on actual resulting sizes, not which
/// half was targeted) already lets through as a legitimate defensive play
fn score_half(
    evaluating_player: u32,
    half_start: usize,
    is_laplacian: bool,
    field: &Field,
    resulting_field: &Field,
) -> f64 {
    let per_slot: f64 = (half_start..half_start + 3)
        .filter_map(|i| field[i].basis.as_ref().map(|basis| (i, basis)))
        .map(|(i, basis)| {
            let once = derivative(basis);
            let result = if is_laplacian { derivative(&once) } else { once };
            score_replacement(evaluating_player, i, &result, field)
        })
        .sum();
    per_slot + strategic_slot_bonus(evaluating_player, resulting_field)
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
/// covers both ways multi_select_phase (events/mousedown_handler.rs) allows
/// combining two bases: two field slots, or one field slot plus one BasisCard from
/// hand (eg. multiplying by a "0" in hand to clear a target outright) -- every
/// other card type is single-target and fully covered by the last match arm below
fn generate_candidates_for(
    player_num: u32,
    hand: &[Card],
    field: &Field,
    turn_number: u32,
) -> Vec<AiMove> {
    let mut moves = vec![];
    
    // Evaluate overall game situation for strategic context
    let situation_bonus = evaluate_game_situation(player_num, hand, field);

    for (i, card) in hand.iter().enumerate() {
        let hand_id = RenderId::from(format!("p{player_num}={i}"));

        match card {
            Card::BasisCard(basis_card) if !matches!(basis_card, BasisCard::Zero) => {
                for target in 0..6 {
                    if field[target].basis.is_none() {
                        let new_basis = Basis::from(*basis_card);
                        let mut resulting_field = field.clone();
                        resulting_field[target] = FieldBasis::new(&new_basis);
                        let wins_immediately =
                            side_is_cleared(&resulting_field, opponent_of(player_num));
                        let hurts_self_or_helps_opponent =
                            hurts_self_or_helps_opponent(player_num, field, &resulting_field);
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={target}"))],
                            score: score_replacement(player_num, target, &new_basis, field)
                                + strategic_slot_bonus(player_num, &resulting_field)
                                + situation_bonus,
                            resulting_field,
                            wins_immediately,
                            hurts_self_or_helps_opponent,
                        });
                    }
                }
            }
            Card::DerivativeCard(DerivativeCard::Nabla) => {
                for half_start in [0usize, 3usize] {
                    let resulting_field = apply_half_to_field(half_start, false, field);
                    let wins_immediately =
                        side_is_cleared(&resulting_field, opponent_of(player_num));
                    let hurts_self_or_helps_opponent =
                        hurts_self_or_helps_opponent(player_num, field, &resulting_field);
                    moves.push(AiMove {
                        clicks: vec![hand_id, RenderId::from(format!("f={half_start}"))],
                        score: score_half(player_num, half_start, false, field, &resulting_field)
                            + situation_bonus,
                        resulting_field,
                        wins_immediately,
                        hurts_self_or_helps_opponent,
                    });
                }
            }
            Card::DerivativeCard(DerivativeCard::Laplacian) if turn_number >= 2 => {
                for half_start in [0usize, 3usize] {
                    let resulting_field = apply_half_to_field(half_start, true, field);
                    let wins_immediately =
                        side_is_cleared(&resulting_field, opponent_of(player_num));
                    let hurts_self_or_helps_opponent =
                        hurts_self_or_helps_opponent(player_num, field, &resulting_field);
                    moves.push(AiMove {
                        clicks: vec![hand_id, RenderId::from(format!("f={half_start}"))],
                        score: score_half(player_num, half_start, true, field, &resulting_field)
                            + situation_bonus,
                        resulting_field,
                        wins_immediately,
                        hurts_self_or_helps_opponent,
                    });
                }
            }
            Card::AlgebraicCard(AlgebraicCard::Div | AlgebraicCard::Mult) => {
                // field+field Mult/Div is deprecated -- multi_select_phase
                // (events/mousedown_handler.rs) now only accepts ONE field slot per
                // Mult/Div play (a second field click is silently dropped there), so
                // a field+field candidate here would be a move the AI could choose
                // but could never actually complete: replaying its clicks would
                // leave `selected` one item short of has_at_least_2_basis, so the
                // Multidone click would never trigger the commit, stranding the
                // turn. Only field+hand-card combinations are generated below,
                // matching what multi_select_phase actually allows: one field slot
                // plus one BasisCard from hand as the two operands, consuming the
                // hand card without freeing up a second field slot. No same-side
                // restriction applies here, since only one operand is even a field
                // slot -- every other single-target card can already aim at either
                // side, and this is no different. Tried in both (field, hand) and
                // (hand, field) order, since Div is order-sensitive
                // (numerator/denominator) even though Mult isn't
                for (hand_index, hand_card) in hand.iter().enumerate() {
                    let hand_basis_card = match hand_card {
                        Card::BasisCard(basis_card) => *basis_card,
                        _ => continue,
                    };
                    let hand_basis = Basis::from(hand_basis_card);
                    let hand_click = RenderId::from(format!("p{player_num}={hand_index}"));
                    for target in 0..6 {
                        let field_basis = match &field[target].basis {
                            Some(basis) => basis.clone(),
                            None => continue,
                        };
                        let field_click = RenderId::from(format!("f={target}"));
                        let orderings = [
                            (
                                vec![field_basis.clone(), hand_basis.clone()],
                                vec![hand_id, field_click, hand_click, RenderId::Multidone],
                            ),
                            (
                                vec![hand_basis.clone(), field_basis.clone()],
                                vec![hand_id, hand_click, field_click, RenderId::Multidone],
                            ),
                        ];
                        for (bases, clicks) in orderings {
                            let result = apply_multi_card(card, bases);
                            let mut resulting_field = field.clone();
                            resulting_field[target] = if result.is_num(0) {
                                FieldBasis::none()
                            } else {
                                FieldBasis::new(&result)
                            };
                            // creating 0 (×0 or ÷0) against an opponent slot clears
                            // it outright -- score_replacement already scores that
                            // at +200 (see "cleared an opponent slot entirely"), so
                            // no separate bonus is added here on top of it
                            let score = score_replacement(player_num, target, &result, field)
                                + strategic_slot_bonus(player_num, &resulting_field)
                                + situation_bonus;
                            let wins_immediately =
                                side_is_cleared(&resulting_field, opponent_of(player_num));
                            let hurts_self_or_helps_opponent =
                                hurts_self_or_helps_opponent(player_num, field, &resulting_field);
                            moves.push(AiMove {
                                clicks,
                                score,
                                resulting_field,
                                wins_immediately,
                                hurts_self_or_helps_opponent,
                            });
                        }
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
                        let wins_immediately =
                            side_is_cleared(&resulting_field, opponent_of(player_num));
                        let hurts_self_or_helps_opponent =
                            hurts_self_or_helps_opponent(player_num, field, &resulting_field);
                        
                        // Strategic evaluation for Limit cards
                        let limit_bonus = if let Card::LimitCard(limit_card) = card {
                            evaluate_limit_strategy(player_num, target, limit_card, &result, field, &resulting_field)
                        } else {
                            0.0
                        };
                        
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={target}"))],
                            score: score_replacement(player_num, target, &result, field)
                                + strategic_slot_bonus(player_num, &resulting_field)
                                + limit_bonus
                                + situation_bonus,
                            resulting_field,
                            wins_immediately,
                            hurts_self_or_helps_opponent,
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
/// handling for the AI's symmetric case). Includes Mult/Div: leaving them out of
/// this estimate meant the AI never noticed a human reply that multiplies/divides
/// its way to a strong follow-up (or a win missed here would still be caught by
/// filter_out_losing_moves' own always-exhaustive check, but a merely-strong reply
/// wouldn't be, undervaluing the danger of the move that allowed it). The same-side
/// restriction on field+field pairs (see generate_candidates_for) already keeps
/// this affordable enough for apply_lookahead's Hard-difficulty worst case (see
/// test_hard_ai_decision_completes_quickly_in_worst_case)
fn best_reply_score(player_num: u32, hand: &[Card], field: &Field, turn_number: u32) -> f64 {
    generate_candidates_for(player_num, hand, field, turn_number)
        .iter()
        .map(|mv| mv.score)
        .fold(f64::MIN, f64::max)
        .max(0.0)
}

/// removes candidates that would let the opponent win outright on their very next
/// turn, unless EVERY candidate shares that fate (in which case the loss is
/// unavoidable regardless of what's picked, so there's nothing safe to filter down
/// to -- normal scoring proceeds among the doomed options instead, eg. to at least
/// maximise damage before losing). Exhaustive (Mult/Div included, via
/// has_winning_move) since missing a lethal reply here would be a real correctness
/// bug, not an acceptable approximation. This is priority tier 2 ("prevent the
/// opponent's immediate win") -- applied for every difficulty, before any of the
/// softer, difficulty-scaled scoring below, since walking into an avoidable loss
/// isn't a "weaker style of play", it's just a mistake
fn filter_out_losing_moves(
    candidates: Vec<AiMove>,
    opponent_hand: &[Card],
    turn_number: u32,
) -> Vec<AiMove> {
    let is_safe: Vec<bool> = candidates
        .iter()
        .map(|mv| !has_winning_move(1, opponent_hand, &mv.resulting_field, turn_number + 1))
        .collect();

    if is_safe.iter().any(|&safe| safe) {
        candidates
            .into_iter()
            .zip(is_safe)
            .filter_map(|(mv, safe)| safe.then_some(mv))
            .collect()
    } else {
        candidates
    }
}

/// how many of the AI's top-scored candidates get the (more expensive) opponent
/// lookahead applied on Medium -- bounds the search since Medium doesn't need to be
/// perfect, just reasonably aware. Hard uses usize::MAX (every candidate) instead:
/// with only a capped pool, a candidate ranked just outside it could still end up
/// chosen (nothing else beat its unadjusted score) with its real risk never having
/// been checked at all -- which is exactly how Hard could still walk into a bad
/// trade despite having lookahead. Hard is expected to always find the true best
/// move, so it must check all of them; the cheap (no Mult/Div) inner search in
/// best_reply_score is what keeps that affordable
const MEDIUM_LOOKAHEAD_POOL: usize = 15;
/// how heavily the opponent's best reply weighs against the AI's own immediate gain
/// when ranking moves -- increased to make AI more defensive and strategic
const LOOKAHEAD_WEIGHT: f64 = 1.5;

/// re-ranks the AI's best candidates (up to `pool` of them, by current score) by
/// subtracting a weighted estimate of the human's best reply one ply ahead, so the
/// AI stops walking into moves that look good immediately but hand the opponent an
/// even better follow-up (eg. clearing an opponent slot while leaving one of its own
/// exposed to an easy kill next turn -- the original one-ply-only heuristic couldn't
/// see this at all)
fn apply_lookahead(
    mut candidates: Vec<AiMove>,
    opponent_hand: &[Card],
    turn_number: u32,
    pool: usize,
) -> Vec<AiMove> {
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    for mv in candidates.iter_mut().take(pool) {
        let reply = best_reply_score(1, opponent_hand, &mv.resulting_field, turn_number + 1);
        mv.score -= LOOKAHEAD_WEIGHT * reply;
    }
    candidates
}

// --- Hard difficulty: genuine minimax search with alpha-beta pruning ------------
//
// apply_lookahead (above) is a heuristic: it scores each of the AI's own
// candidates, then subtracts a *weighted estimate* of the single best reply the
// opponent could make -- never checking what happens after THAT, and never
// re-deriving its own future moves as anything other than that same one-shot
// heuristic. It can miss real multi-move tactics (a trade that only pays off two
// or three moves later) and has no principled way to compare "this move scores
// well but leaves a bad follow-up" against "this move scores modestly but forces
// a winning sequence".
//
// The search below replaces that ranking step for Hard difficulty with a proper
// minimax: it plays out actual alternating turns to a search horizon, using
// score_replacement/strategic_slot_bonus only for MOVE ORDERING (so alpha-beta
// prunes effectively) and evaluate_position as the static leaf evaluator. Every
// hard safety rule (self-defeat, immediate loss, hurts-self/helps-opponent,
// progress, non-strengthening -- see apply_hard_filters) still applies at every
// simulated AI turn along the way, not just the root, so the search never credits
// a line with a move the real AI would never actually choose when it got there.
//
// Both hands are held static for the whole search: no card is removed from hand
// after being "played" in a simulated line, and no future draw is modelled. The
// real deck order isn't something the AI should be treated as knowing in advance,
// and the actual chosen move is always executed for real afterward through the
// normal game engine (which DOES consume/redraw correctly) regardless of what
// this search assumed -- this is a lookahead heuristic on which cards get USED,
// not a claim about exact future hand contents.

/// how many of the highest-move-ordered candidates get explored at each minimax
/// node beyond the root -- without this, the search tree's size explodes
/// combinatorially (branching factor ^ depth), since generating candidates here
/// isn't free (each one builds a real symbolic Basis). The root itself (the AI's
/// actual move choice, in choose_move_via_minimax) is never capped -- only the
/// simulated replies/follow-ups explored while evaluating each root candidate are
const MINIMAX_BRANCH_CAP: usize = 20;
/// within MINIMAX_BRANCH_CAP, how many slots are reserved for the best-scored
/// BUILD/REFRESH-style candidates (see is_attacking_move) even if every one of
/// them scores lower than the top ATTACK-style candidates. A plain top-N-by-score
/// truncation can starve the search of exactly the moves worth comparing against
/// an attack: a setup play that pays off two or three plies later never gets a
/// chance to prove it, because it never even enters the pruned tree. This doesn't
/// change the total number of candidates explored per node, only which ones --
/// same cost, broader coverage
const MINIMAX_MIN_NON_ATTACK_SLOTS: usize = 4;
/// hard ceiling on search depth in plies (individual moves, not full rounds).
/// Deliberately shallow: both hands are held static across the whole search (see
/// this module's own doc), with only the one card actually played at each
/// simulated ply removed (see hand_after_move) -- no redraw is ever modelled. In
/// the real game a hand redraws up to 7 cards at the end of EVERY turn, so by
/// depth 12 (6 of the AI's own simulated turns, 6 of the opponent's) the search
/// is reasoning about a hand of leftover cards that would never actually exist --
/// a scenario neither side ever draws a single new card for six turns straight --
/// which produces increasingly fictional, noisy signal rather than useful
/// lookahead. Empirically, a depth-12 cap was reached almost every turn well
/// within budget even for typical (non-worst-case) hands -- the search wasn't
/// time-constrained at all, it was spending its generous time budget exploring
/// ever-deeper fiction instead of examining more real options at a depth where
/// the static-hand assumption is still a reasonable approximation. Shallower and
/// wider (see MINIMAX_BRANCH_CAP) trades speculative depth for a more thorough
/// look at the moves actually worth comparing
const MINIMAX_MAX_DEPTH: u32 = 6;
/// how long Hard difficulty may spend searching for a single real move in
/// production. Iterative deepening (see choose_move_via_minimax) means a
/// smaller/faster position simply finishes early rather than wasting this whole
/// budget. choose_move takes the budget as a parameter rather than hard-coding
/// this everywhere specifically so tests can pass a much smaller one -- a
/// randomized sweep across thousands of positions at up to 5 real seconds each
/// would make the test suite itself unusably slow, even though none of that time
/// is needed to verify the properties those tests actually check
const DEFAULT_MINIMAX_TIME_BUDGET: std::time::Duration = std::time::Duration::from_millis(5000);

/// position-only evaluation used at the minimax search horizon, where there's no
/// further move to score -- unlike score_replacement/strategic_slot_bonus, which
/// score a MOVE (a transition from one field to another), this needs to score a
/// STATE directly. Rather than inventing a separate hand-rolled formula (an
/// earlier version of this function did exactly that, weighted toward occupied
/// slot count with total complexity as a tie-breaker) and risking it correlating
/// poorly with actual winning chances, this reuses the SAME move-scoring
/// machinery every other tier and every other difficulty already relies on: the
/// best score_replacement/strategic_slot_bonus-rated move ai_player_num could
/// make right now from this position (see best_reply_score). A position where the
/// AI's own best available move already scores well (eg. an easy slot clear
/// sitting right there) really is a good position, and this evaluator agrees for
/// the same reason Medium's ranking does -- rather than the search's leaves
/// judging "good" by a different, untested standard than its own move ordering
/// and every other difficulty use throughout this file. Only ever called on a
/// non-terminal position (see terminal_outcome, checked first at every node), so
/// it never needs to handle either side already being at zero itself
fn evaluate_position(ai_player_num: u32, ai_hand: &[Card], field: &Field, turn_number: u32) -> f64 {
    best_reply_score(ai_player_num, ai_hand, field, turn_number)
}

/// `hand` with whichever card(s) `mv` used removed -- the hand a side has
/// available for its NEXT simulated turn along this specific line. Mirrors
/// end_turn's own card-removal logic (events/mousedown_handler.rs): every
/// "player" click in `mv.clicks` (the operator card, and for a field+hand-card
/// Mult/Div move, the BasisCard operand too) corresponds to a card actually
/// consumed. Without this, a card played at one simulated ply would still show up
/// as available two plies later in the same line -- not just an unlikely
/// coincidence but a systematic distortion, since the search would always "find"
/// the same good card still sitting in hand for a follow-up that in reality would
/// have already been spent
fn hand_after_move(hand: &[Card], mv: &AiMove) -> Vec<Card> {
    let mut used_indices: Vec<usize> = mv
        .clicks
        .iter()
        .filter(|id| id.is_player())
        .map(|id| id.key_val().1)
        .collect();
    used_indices.sort_unstable();
    used_indices.reverse();
    let mut remaining = hand.to_vec();
    for i in used_indices {
        remaining.remove(i);
    }
    remaining
}

/// true if `mv` measurably reduces the ACTING player's opponent -- either fewer
/// occupied slots on their side, or a smaller expression in one that's still
/// occupied. A rough ATTACK/not-ATTACK split (see MINIMAX_MIN_NON_ATTACK_SLOTS):
/// "not attacking" covers everything else a turn can do here -- strengthening or
/// reorganizing one's own side, since this game has no way to skip a turn, so
/// there's no separate WAIT category to distinguish from BUILD/REFRESH
fn is_attacking_move(mv: &AiMove, field: &Field, acting_player: u32) -> bool {
    (0..6).any(|i| {
        field_owner(i) != acting_player && {
            let old_size = field[i].basis.as_ref().map(basis_size).unwrap_or(0);
            let new_size = mv.resulting_field[i].basis.as_ref().map(basis_size).unwrap_or(0);
            new_size < old_size
        }
    })
}

/// caps `candidates` to MINIMAX_BRANCH_CAP for exploration at a minimax node
/// beyond the root, reserving MINIMAX_MIN_NON_ATTACK_SLOTS of that cap for the
/// best-scored non-attacking (BUILD/REFRESH-style) candidates -- see
/// MINIMAX_MIN_NON_ATTACK_SLOTS's doc on why a plain top-N-by-score truncation
/// isn't enough. A no-op (aside from sorting) when there's nothing to trim
fn select_branch_candidates(
    mut candidates: Vec<AiMove>,
    field: &Field,
    acting_player: u32,
) -> Vec<AiMove> {
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    if candidates.len() <= MINIMAX_BRANCH_CAP {
        return candidates;
    }
    // `candidates` is already sorted, and partition preserves relative order, so
    // both resulting groups stay sorted too
    let (attacking, non_attacking): (Vec<AiMove>, Vec<AiMove>) = candidates
        .into_iter()
        .partition(|mv| is_attacking_move(mv, field, acting_player));
    let non_attack_reserved: Vec<AiMove> = non_attacking
        .into_iter()
        .take(MINIMAX_MIN_NON_ATTACK_SLOTS)
        .collect();
    let attack_slots = MINIMAX_BRANCH_CAP.saturating_sub(non_attack_reserved.len());
    let mut selected: Vec<AiMove> = attacking.into_iter().take(attack_slots).collect();
    selected.extend(non_attack_reserved);
    selected.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    selected
}

/// Some(final score, from ai_player_num's point of view) if `mv` ends the game
/// outright, None if the game continues past this move. Checked before recursing
/// into a child so a terminal move short-circuits instead of wastefully trying to
/// generate further moves for an already-finished game -- and so a move that
/// clears the ACTING player's own side is correctly scored as a loss for THEM even
/// if it also cleared the other side in the same move, matching next_turn's
/// own-side-checked-first rule (see filter_out_self_defeating_moves's doc)
fn terminal_outcome(mv: &AiMove, acting_player: u32, ai_player_num: u32) -> Option<f64> {
    if side_is_cleared(&mv.resulting_field, acting_player) {
        return Some(if acting_player == ai_player_num {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if mv.wins_immediately {
        return Some(if acting_player == ai_player_num {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        });
    }
    None
}

/// outcome of running the AI's own hard safety/progress filters (tiers 0, 2-5 --
/// see choose_move's doc) against a fresh candidate set: either one of them wins
/// outright (tier 1, checked in between tiers 0 and 2), or here's what's left
/// after every filter that applies regardless of difficulty
enum HardFilterResult {
    Win(AiMove),
    Filtered(Vec<AiMove>),
}

/// applies every hard rule the real AI always applies before any difficulty-scaled
/// ranking -- tier 0 (never self-defeat), tier 1 (always take an outright win),
/// tier 2 (never hand the opponent an immediate win), tier 3 (never hurt self/help
/// opponent), tier 4 (always make progress), tier 5 (never strengthen the
/// opponent) -- shared between choose_move's own root decision and minimax's
/// simulated future AI turns, so the search never credits a line with a move the
/// real AI would never actually choose once it got there
fn apply_hard_filters(
    candidates: Vec<AiMove>,
    opponent_hand: &[Card],
    turn_number: u32,
    field: &Field,
) -> HardFilterResult {
    let candidates = filter_out_self_defeating_moves(candidates);
    if let Some(winning_index) = candidates.iter().position(|mv| mv.wins_immediately) {
        let mut candidates = candidates;
        return HardFilterResult::Win(candidates.remove(winning_index));
    }
    let candidates = filter_out_losing_moves(candidates, opponent_hand, turn_number);
    let candidates = filter_out_moves_that_hurt_self_or_help_opponent(candidates);
    let candidates = filter_out_non_progressive_moves(candidates, field);
    let candidates = filter_out_opponent_strengthening_moves(candidates, field);
    HardFilterResult::Filtered(candidates)
}

/// recursive minimax search with alpha-beta pruning, maximizing for
/// `ai_player_num` and minimizing for the other player. Returns None if the time
/// budget ran out before this subtree could be fully evaluated, so the caller
/// (choose_move_via_minimax) can discard an incomplete iterative-deepening pass
/// rather than act on a partial (and therefore unreliable) result
#[allow(clippy::too_many_arguments)]
fn minimax(
    field: &Field,
    ai_player_num: u32,
    acting_player: u32,
    ai_hand: &[Card],
    opponent_hand: &[Card],
    turn_number: u32,
    depth_remaining: u32,
    mut alpha: f64,
    mut beta: f64,
    deadline: std::time::Instant,
) -> Option<f64> {
    if std::time::Instant::now() >= deadline {
        return None;
    }
    if depth_remaining == 0 {
        return Some(evaluate_position(ai_player_num, ai_hand, field, turn_number));
    }

    let is_maximizing = acting_player == ai_player_num;
    let candidates = if is_maximizing {
        // a simulated future AI turn must obey the same hard rules the real AI
        // always applies -- otherwise the search could credit a line with a move
        // (eg. one that hurts the AI's own field) the real AI would never actually
        // make when it got there
        match apply_hard_filters(
            generate_candidates_for(acting_player, ai_hand, field, turn_number),
            opponent_hand,
            turn_number,
            field,
        ) {
            HardFilterResult::Win(_) => return Some(f64::INFINITY),
            HardFilterResult::Filtered(candidates) => candidates,
        }
    } else {
        // the opponent isn't bound by the AI's own design rules -- a minimax
        // search should conservatively assume they play their objectively best
        // available move, not one artificially narrowed by the AI's own filters
        generate_candidates_for(acting_player, opponent_hand, field, turn_number)
    };

    if candidates.is_empty() {
        // no legal move for whoever's turn this is -- treat the position as final
        // for search purposes rather than modelling a forfeited turn (a rare edge
        // case; the real game engine's own forfeit handling covers it live)
        return Some(evaluate_position(ai_player_num, ai_hand, field, turn_number));
    }

    let candidates = select_branch_candidates(candidates, field, acting_player);

    let mut best = if is_maximizing {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    };

    for mv in &candidates {
        let score = match terminal_outcome(mv, acting_player, ai_player_num) {
            Some(score) => score,
            None => {
                // whichever side just moved no longer has the card(s) it just
                // used, for the rest of THIS simulated line -- see
                // hand_after_move's doc on why this matters
                let next_ai_hand;
                let next_opponent_hand;
                let (next_ai_hand_ref, next_opponent_hand_ref): (&[Card], &[Card]) =
                    if is_maximizing {
                        next_ai_hand = hand_after_move(ai_hand, mv);
                        (&next_ai_hand, opponent_hand)
                    } else {
                        next_opponent_hand = hand_after_move(opponent_hand, mv);
                        (ai_hand, &next_opponent_hand)
                    };
                minimax(
                    &mv.resulting_field,
                    ai_player_num,
                    opponent_of(acting_player),
                    next_ai_hand_ref,
                    next_opponent_hand_ref,
                    turn_number + 1,
                    depth_remaining - 1,
                    alpha,
                    beta,
                    deadline,
                )?
            }
        };
        if is_maximizing {
            best = best.max(score);
            alpha = alpha.max(best);
        } else {
            best = best.min(score);
            beta = beta.min(best);
        }
        if beta <= alpha {
            break;
        }
    }
    Some(best)
}

/// picks the AI's move via genuine minimax search with alpha-beta pruning and
/// iterative deepening (see the module doc above), replacing apply_lookahead
/// entirely for Hard difficulty. `candidates` have already passed every hard
/// filter (see apply_hard_filters) -- this only decides which of THOSE is truly
/// best, by searching as many plies ahead as the time budget allows. Each
/// iterative-deepening pass reorders the root candidates so the previous pass's
/// best move is tried first at the next depth, giving alpha-beta a strong initial
/// bound to prune against; an interrupted pass is discarded outright rather than
/// trusted, so the final answer always reflects a depth that was fully evaluated
fn choose_move_via_minimax(
    mut candidates: Vec<AiMove>,
    ai_hand: &[Card],
    opponent_hand: &[Card],
    turn_number: u32,
    time_budget: std::time::Duration,
) -> AiMove {
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    let deadline = std::time::Instant::now() + time_budget;

    let mut best_index = 0;
    let mut depth = 1;
    while depth <= MINIMAX_MAX_DEPTH && std::time::Instant::now() < deadline {
        let mut alpha = f64::NEG_INFINITY;
        let beta = f64::INFINITY;
        let mut this_pass_best: Option<(usize, f64)> = None;
        let mut timed_out = false;

        for (i, mv) in candidates.iter().enumerate() {
            let score = match terminal_outcome(mv, AI_PLAYER_NUM, AI_PLAYER_NUM) {
                Some(score) => score,
                None => {
                    // the AI no longer has the card it's about to play, for the
                    // rest of this simulated line -- see hand_after_move's doc
                    let ai_hand_after_root_move = hand_after_move(ai_hand, mv);
                    match minimax(
                        &mv.resulting_field,
                        AI_PLAYER_NUM,
                        opponent_of(AI_PLAYER_NUM),
                        &ai_hand_after_root_move,
                        opponent_hand,
                        turn_number + 1,
                        depth,
                        alpha,
                        beta,
                        deadline,
                    ) {
                        Some(score) => score,
                        None => {
                            timed_out = true;
                            break;
                        }
                    }
                }
            };
            if this_pass_best.is_none_or(|(_, best_score)| score > best_score) {
                this_pass_best = Some((i, score));
            }
            alpha = alpha.max(score);
        }

        if timed_out {
            break;
        }
        if let Some((i, _)) = this_pass_best {
            candidates.swap(0, i);
            best_index = 0;
        }
        depth += 1;
    }

    candidates.remove(best_index)
}

/// removes candidates that would immediately end the game in the AI's own defeat
/// (clearing every slot the AI owns), unless EVERY candidate does this (nothing left
/// to filter down to). Checked before even tier 1 (an immediate win): in the rare
/// case a single Mult/Div move clears an opponent slot AND the AI's own last slot at
/// once, next_turn's win check (see side_is_cleared) tests the *mover's own* side
/// first, so that move is a loss regardless of what it also did to the opponent's
/// side. Previously this was only discouraged via score_replacement's fixed -50
/// penalty for clearing an own slot, which a large enough simultaneous
/// opponent-clear bonus (+100) could outweigh -- letting the AI choose a move that
/// looked good on net score but instantly lost the game by its own hand. That's a
/// hard rule, not a soft preference, so it's a filter here, not a score adjustment
fn filter_out_self_defeating_moves(candidates: Vec<AiMove>) -> Vec<AiMove> {
    let is_safe: Vec<bool> = candidates
        .iter()
        .map(|mv| !side_is_cleared(&mv.resulting_field, AI_PLAYER_NUM))
        .collect();

    if is_safe.iter().any(|&safe| safe) {
        candidates
            .into_iter()
            .zip(is_safe)
            .filter_map(|(mv, safe)| safe.then_some(mv))
            .collect()
    } else {
        candidates
    }
}

/// removes candidates that attack the AI's own field or strengthen the opponent's
/// (see hurts_self_or_helps_opponent), unless every remaining candidate shares the
/// problem. This is a hard rule, not a scoring nudge: the AI must always be able to
/// attack the opponent's field and defend/strengthen its own, and must never do the
/// reverse. Placed after tier 1's immediate-win check in choose_move (not before)
/// so it can never filter out a genuine win -- eg. a Mult/Div combo that sacrifices
/// one of the AI's own slots to clear the opponent's last one is still a win and
/// must always be taken -- and after tier 2's immediate-loss check, so a move
/// that's the only way to survive the opponent's next turn is never discarded here
/// either; avoiding an actual loss always outranks this
fn filter_out_moves_that_hurt_self_or_help_opponent(candidates: Vec<AiMove>) -> Vec<AiMove> {
    let is_safe: Vec<bool> = candidates
        .iter()
        .map(|mv| !mv.hurts_self_or_helps_opponent)
        .collect();

    if is_safe.iter().any(|&safe| safe) {
        candidates
            .into_iter()
            .zip(is_safe)
            .filter_map(|(mv, safe)| safe.then_some(mv))
            .collect()
    } else {
        candidates
    }
}

/// removes candidates that don't make progress toward winning (don't reduce
/// opponent's slot count or complexity), unless every candidate is non-progressive.
/// This ensures the AI always chooses moves that advance toward victory when possible.
fn filter_out_non_progressive_moves(candidates: Vec<AiMove>, field: &Field) -> Vec<AiMove> {
    let opponent_initial_count: u32 = (0..6)
        .filter(|&i| field_owner(i) == 1)
        .filter(|&i| field[i].basis.is_some())
        .count() as u32;
    
    let is_progressive: Vec<bool> = candidates
        .iter()
        .map(|mv| {
            let opponent_final_count: u32 = (0..6)
                .filter(|&i| field_owner(i) == 1)
                .filter(|&i| mv.resulting_field[i].basis.is_some())
                .count() as u32;
            // Progressive if it reduces opponent's slot count or simplifies opponent's slots
            opponent_final_count < opponent_initial_count || 
            (0..6).any(|i| {
                field_owner(i) == 1 && 
                field[i].basis.as_ref().map(basis_size).unwrap_or(0) > 
                mv.resulting_field[i].basis.as_ref().map(basis_size).unwrap_or(0)
            })
        })
        .collect();

    if is_progressive.iter().any(|&prog| prog) {
        candidates
            .into_iter()
            .zip(is_progressive)
            .filter_map(|(mv, prog)| prog.then_some(mv))
            .collect()
    } else {
        candidates
    }
}

/// evaluates Limit cards strategically, particularly focusing on:
/// - Using lim(x→∞) on x to clear opponent's slot (x→∞ = ∞, but lim(x→∞) of x is ∞, which may not clear)
/// - Using limit cards to simplify own field when in danger
/// - Evaluating whether limit operations are beneficial strategically
fn evaluate_limit_strategy(
    player_num: u32,
    target: usize,
    limit_card: &LimitCard,
    result: &Basis,
    field: &Field,
    resulting_field: &Field,
) -> f64 {
    let is_opponent_side = field_owner(target) != player_num;
    let old_size = field[target].basis.as_ref().map(basis_size).unwrap_or(0);
    let new_size = if result.is_num(0) { 0 } else { basis_size(result) };
    
    // Check if this clears the slot (result is 0 or effectively simple)
    let clears_slot = result.is_num(0) || new_size < old_size;
    
    // Bonus for clearing opponent's slot with limit
    if is_opponent_side && clears_slot {
        return 150.0;
    }
    
    // Penalty for clearing own slot unless it saves from immediate loss
    if !is_opponent_side && clears_slot {
        // Check if we're in danger (few slots remaining)
        let own_slots_remaining: u32 = (0..6)
            .filter(|&i| field_owner(i) == player_num)
            .filter(|&i| field[i].basis.is_some())
            .count() as u32;
        
        if own_slots_remaining <= 2 {
            // In danger, clearing own slot is very bad
            return -200.0;
        } else {
            // Not in immediate danger, still penalize
            return -50.0;
        }
    }
    
    // Special case: lim(x→∞) on x-like expressions
    // If the result simplifies to something very simple, reward it
    if new_size <= 2 && old_size > 2 {
        return 50.0;
    }
    
    0.0
}

/// evaluates the overall game situation and provides strategic bonuses/penalties
/// This helps the AI understand when to be aggressive vs defensive
fn evaluate_game_situation(player_num: u32, hand: &[Card], field: &Field) -> f64 {
    let opponent_num = opponent_of(player_num);
    
    // Count remaining slots for both players
    let own_slots_remaining: u32 = (0..6)
        .filter(|&i| field_owner(i) == player_num)
        .filter(|&i| field[i].basis.is_some())
        .count() as u32;
    
    let opponent_slots_remaining: u32 = (0..6)
        .filter(|&i| field_owner(i) == opponent_num)
        .filter(|&i| field[i].basis.is_some())
        .count() as u32;
    
    let mut situation_score = 0.0;
    
    // If we're in danger (few slots), be more defensive
    if own_slots_remaining <= 2 {
        situation_score -= 100.0; // Penalize risky moves
    }
    
    // If opponent is in danger, be more aggressive
    if opponent_slots_remaining <= 2 {
        situation_score += 100.0; // Reward aggressive moves
    }
    
    // Check for Mult/Div cards in hand for strategic plays
    let has_mult_div = hand.iter().any(|c| matches!(c, Card::AlgebraicCard(AlgebraicCard::Mult | AlgebraicCard::Div)));
    let has_zero = hand.iter().any(|c| matches!(c, Card::BasisCard(BasisCard::Zero)));
    
    // If we have Mult/Div and opponent has simple expressions, bonus for aggressive play
    if has_mult_div && opponent_slots_remaining > 0 {
        let opponent_has_simple = (0..6).any(|i| {
            field_owner(i) == opponent_num && 
            field[i].basis.as_ref().map(basis_size).unwrap_or(0) <= 2
        });
        if opponent_has_simple {
            situation_score += 50.0;
        }
    }
    
    // If we have 0 in hand and can use it strategically, bonus
    if has_zero && has_mult_div {
        situation_score += 30.0;
    }
    
    situation_score
}

/// removes candidates that directly strengthen the opponent's field (increase
/// opponent's slot complexity), unless every candidate does this. This is a
/// defensive filter to ensure the AI never voluntarily makes the opponent stronger.
fn filter_out_opponent_strengthening_moves(candidates: Vec<AiMove>, field: &Field) -> Vec<AiMove> {
    let is_safe: Vec<bool> = candidates
        .iter()
        .map(|mv| {
            // Safe if it doesn't increase any opponent slot's complexity
            !(0..6).any(|i| {
                field_owner(i) == 1 && 
                field[i].basis.as_ref().map(basis_size).unwrap_or(0) < 
                mv.resulting_field[i].basis.as_ref().map(basis_size).unwrap_or(0)
            })
        })
        .collect();

    if is_safe.iter().any(|&safe| safe) {
        candidates
            .into_iter()
            .zip(is_safe)
            .filter_map(|(mv, safe)| safe.then_some(mv))
            .collect()
    } else {
        candidates
    }
}

/// difficulty-scaled selection. Every hard rule (see apply_hard_filters) applies
/// regardless of difficulty: (1) never self-defeat, (2) always take an outright
/// win, (3) never hand the opponent an immediate win next turn if avoidable, (4)
/// never attack our own field or strengthen the opponent's if avoidable, (5)
/// always make progress toward winning if possible, (6) never directly strengthen
/// the opponent if avoidable. Among whatever survives those, Hard hands off to a
/// genuine minimax search (see choose_move_via_minimax) that always finds the
/// true best move within its search horizon; Medium looks ahead one ply but only
/// across its top MEDIUM_LOOKAHEAD_POOL candidates and picks randomly from a
/// shrinking top slice of that adjusted ranking (reasonably aware, not required to
/// be perfect); Easy skips lookahead entirely and stays mostly random so it
/// remains reliably beatable. `minimax_time_budget` is only consulted for Hard --
/// see DEFAULT_MINIMAX_TIME_BUDGET's doc on why this is a parameter instead of
/// always using that constant directly
fn choose_move(
    candidates: Vec<AiMove>,
    ai_hand: &[Card],
    opponent_hand: &[Card],
    difficulty: AiDifficulty,
    turn_number: u32,
    field: &Field,
    minimax_time_budget: std::time::Duration,
) -> AiMove {
    let candidates = match apply_hard_filters(candidates, opponent_hand, turn_number, field) {
        HardFilterResult::Win(winning_move) => return winning_move,
        HardFilterResult::Filtered(candidates) => candidates,
    };

    if matches!(difficulty, AiDifficulty::Hard) {
        return choose_move_via_minimax(
            candidates,
            ai_hand,
            opponent_hand,
            turn_number,
            minimax_time_budget,
        );
    }

    let mut candidates = match difficulty {
        AiDifficulty::Medium => {
            apply_lookahead(candidates, opponent_hand, turn_number, MEDIUM_LOOKAHEAD_POOL)
        }
        AiDifficulty::Easy => candidates,
        AiDifficulty::Hard => unreachable!("Hard already returned above"),
    };
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    let mut rng = rand::thread_rng();

    let index = match difficulty {
        AiDifficulty::Medium => {
            // Reduced randomness for Medium - choose from top 20% instead of 34%
            let pool = ((candidates.len() as f64) * 0.20).ceil().max(1.0) as usize;
            rng.gen_range(0..pool.min(candidates.len()))
        }
        AiDifficulty::Easy => {
            // Easy remains mostly random for playability
            if rng.gen_bool(0.25) {
                let pool = candidates.len().min(3);
                rng.gen_range(0..pool)
            } else {
                rng.gen_range(0..candidates.len())
            }
        }
        AiDifficulty::Hard => unreachable!("Hard already returned above"),
    };
    candidates.remove(index)
}

#[cfg(test)]
mod perf_tests {
    use super::*;
    use crate::basis::builders::SqrtBasisNode;
    use crate::math::fraction::Fraction;
    use crate::math::logarithm::logarithm;
    use crate::math::util::function_composition;

    /// builds a Basis nested `depth` levels deep via direct BasisNode construction
    /// (bypassing PowBasisNode's builder, which may normalize/flatten nested
    /// operands and so wouldn't reliably produce real structural depth) -- used to
    /// exercise ComputeDepthGuard's bail-out path in logarithm()/
    /// function_composition(). Confirmed by direct experiment (temporarily
    /// stripping the guard) that logarithm() genuinely stack-overflows and aborts
    /// the process on input this shape at a depth in the thousands -- the depth
    /// used by the tests below (100, comfortably past MAX_COMPUTE_DEPTH's 48) is
    /// deep enough to force the guard to actually bail out mid-recursion, while
    /// staying shallow enough that the bailed-out clone of the remaining subtree
    /// can't itself become a second, unrelated stack-depth problem
    fn deeply_nested_basis(depth: u32) -> Basis {
        let mut basis = Basis::x();
        for _ in 0..depth {
            basis = Basis::BasisNode(BasisNode {
                coefficient: Fraction::from(1),
                operator: BasisOperator::Pow(Fraction { n: 1, d: 1 }),
                operands: vec![basis],
            });
        }
        basis
    }

    /// logarithm() recurses through Pow nodes with no depth check of its own
    /// (unlike derivative/integral/inverse, which all guard themselves) -- a
    /// sufficiently nested expression previously would have overflowed the stack
    /// instead of returning. The AI's exhaustive search (tries every card against
    /// every field slot every turn) hits this far more easily than a human would,
    /// since expressions grow more nested the longer a game runs
    #[test]
    fn test_logarithm_does_not_overflow_on_deeply_nested_input() {
        let basis = deeply_nested_basis(100);
        // must simply return without panicking/overflowing; the exact bailed-out
        // value isn't the point of this test
        let _ = logarithm(&basis);
    }

    /// same gap, same fix, for function_composition (used by derivative's inverse
    /// rule and by integration-by-parts' LIATE substitution)
    #[test]
    fn test_function_composition_does_not_overflow_on_deeply_nested_input() {
        let basis = deeply_nested_basis(100);
        let _ = function_composition(&basis, &Basis::x());
    }

    /// stacks the hand with the single most expensive card type (Mult/Div) plus
    /// several BasisCards, to stress both of Mult/Div's combination modes at once:
    /// field+field pairs (same-side only, see generate_candidates_for) and
    /// field+hand-card pairs (every occupied field slot combined with every
    /// BasisCard still in hand, tried in both operand orders) -- a hand with no
    /// BasisCards at all (an earlier version of this helper) could never exercise
    /// the field+hand-card branch's own worst case
    fn worst_case_hand() -> Vec<Card> {
        vec![
            Card::AlgebraicCard(AlgebraicCard::Mult),
            Card::AlgebraicCard(AlgebraicCard::Div),
            Card::AlgebraicCard(AlgebraicCard::Mult),
            Card::AlgebraicCard(AlgebraicCard::Div),
            Card::BasisCard(BasisCard::X),
            Card::BasisCard(BasisCard::X2),
            Card::BasisCard(BasisCard::Sin),
        ]
    }

    fn full_field() -> Field {
        let mut field = Field::new();
        for i in 0..6 {
            let basis = match i % 3 {
                0 => Basis::from(1),
                1 => Basis::from(BasisCard::X),
                _ => Basis::from(BasisCard::X2),
            };
            field[i] = FieldBasis::new(&basis);
        }
        field
    }

    /// a field where every slot has been repeatedly integrated several times --
    /// approximates what a real, long-running game's field actually looks like
    /// (genuinely nested expressions from many turns of Integral/Nabla play), unlike
    /// full_field's freshly-dealt 1/x/x^2 values. Used to check the AI's decision
    /// process (including Log, whose missing depth guard was the actual cause of
    /// the AI freezing partway through a game -- see logarithm.rs) against
    /// something structurally closer to what triggers that class of bug in practice
    fn long_game_field() -> Field {
        let mut field = Field::new();
        for i in 0..6 {
            let mut basis = match i % 3 {
                0 => Basis::from(1),
                1 => Basis::from(BasisCard::X),
                _ => Basis::from(BasisCard::X2),
            };
            for _ in 0..10 {
                basis = crate::math::integral::integral(&basis);
            }
            field[i] = FieldBasis::new(&basis);
        }
        field
    }

    /// the AI's full decision process (including Log, whose recursion through
    /// Mult/Div/Pow previously had no depth guard -- see logarithm.rs) must still
    /// respect its time budget against a field shaped like a real, long-running
    /// game rather than a freshly-dealt one. Uses a short budget (not
    /// DEFAULT_MINIMAX_TIME_BUDGET's real 5s) purely so this test itself runs
    /// quickly -- the property being checked (the deadline is actually honoured)
    /// doesn't depend on how long that budget is
    #[test]
    fn test_hard_ai_handles_a_long_games_nested_field_without_freezing() {
        let field = long_game_field();
        let ai_hand = vec![
            Card::AlgebraicCard(AlgebraicCard::Log),
            Card::AlgebraicCard(AlgebraicCard::Log),
            Card::DerivativeCard(DerivativeCard::Derivative),
            Card::DerivativeCard(DerivativeCard::Integral),
            Card::AlgebraicCard(AlgebraicCard::Sqrt),
            Card::AlgebraicCard(AlgebraicCard::Inverse),
            Card::LimitCard(LimitCard::LimPosInf),
        ];
        let opponent_hand = ai_hand.clone();
        let budget = std::time::Duration::from_millis(500);

        let start = std::time::Instant::now();
        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 20);
        assert!(!candidates.is_empty(), "long-game field should still have legal moves");
        let _chosen = choose_move(candidates, &ai_hand, &opponent_hand, AiDifficulty::Hard, 20, &field, budget);
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget + std::time::Duration::from_millis(3000),
            "AI decision against a long-game field took {:?} against a {:?} budget -- \
             the deadline isn't being honoured closely enough, risks looking frozen \
             at the real (much larger) production budget",
            elapsed,
            budget
        );
    }

    /// reproduces the "AI stops" report: both the AI's and opponent's hands stacked
    /// with the most expensive card type combinations (Mult/Div, both against
    /// other field slots and against BasisCards from hand). Does a real,
    /// production-budget Hard decision complete within that budget (plus a small
    /// margin for whatever node was already in flight when the deadline passed) --
    /// eg. does the search's own deadline check actually get hit often enough,
    /// rather than the search wildly overrunning it by continuing to expand
    /// already-started subtrees? This is the actual worst case the real game can
    /// produce (Mult/Div is the single most expensive branch; nothing is more
    /// expensive than having it fully available on both sides of the search)
    #[test]
    fn test_hard_ai_decision_completes_quickly_in_worst_case() {
        let field = full_field();
        let ai_hand = worst_case_hand();
        let opponent_hand = worst_case_hand();
        let budget = DEFAULT_MINIMAX_TIME_BUDGET;

        let start = std::time::Instant::now();
        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4);
        assert!(!candidates.is_empty(), "worst-case hand should still have legal moves");
        let candidate_count = candidates.len();
        let chosen = choose_move(candidates, &ai_hand, &opponent_hand, AiDifficulty::Hard, 4, &field, budget);
        let elapsed = start.elapsed();

        println!(
            "worst-case Hard AI: {} candidates, decided in {:?}, chose {} clicks",
            candidate_count,
            elapsed,
            chosen.clicks.len()
        );
        assert!(
            elapsed < budget + std::time::Duration::from_millis(3000),
            "AI decision took {:?} against a {:?} budget with a worst-case hand -- \
             the deadline isn't being honoured closely enough",
            elapsed,
            budget
        );
    }

    /// priority tier 1: a move that wins outright must always be taken, even when
    /// another candidate (that doesn't win) is also on offer
    #[test]
    fn test_ai_takes_immediate_win_over_other_options() {
        // Field::new() starts every slot occupied (the game's real starting
        // layout) -- slots meant to be empty must be cleared explicitly
        let mut field = Field::new();
        field[3] = FieldBasis::none();
        field[4] = FieldBasis::none();
        // field[5] is the opponent's last remaining slot -- derivative(1) = 0
        // clears it, winning outright
        field[5] = FieldBasis::new(&Basis::from(1));

        let ai_hand = vec![
            Card::AlgebraicCard(AlgebraicCard::Sqrt), // decoy: doesn't win
            Card::DerivativeCard(DerivativeCard::Derivative), // wins if aimed at field[5]
        ];
        let opponent_hand = vec![Card::BasisCard(BasisCard::X)];

        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4);
        assert!(
            candidates.iter().any(|mv| mv.wins_immediately),
            "test setup is wrong: no winning candidate was generated at all"
        );
        let chosen = choose_move(candidates, &ai_hand, &opponent_hand, AiDifficulty::Hard, 4, &field, std::time::Duration::from_millis(200));

        assert!(
            chosen.wins_immediately,
            "AI had a winning move available but chose a different one instead"
        );
    }

    /// the AI's hand can win by multiplying an opponent's last remaining slot by a
    /// "0" BasisCard in hand (anything times zero is zero, clearing the slot) --
    /// generate_candidates_for previously only ever paired two FIELD slots for
    /// Mult/Div, never a field slot with a BasisCard from hand, so this exact move
    /// (and any other field+hand-card Mult/Div combination) was invisible to the
    /// AI's search even when it was the only winning move available
    #[test]
    fn test_ai_wins_by_multiplying_opponents_last_slot_by_a_zero_from_hand() {
        let mut field = Field::new();
        field[4] = FieldBasis::none();
        field[5] = FieldBasis::none();
        // field[3] is the opponent's last remaining slot
        field[3] = FieldBasis::new(&Basis::from(BasisCard::X));

        let ai_hand = vec![
            Card::AlgebraicCard(AlgebraicCard::Sqrt), // decoy: doesn't win
            Card::AlgebraicCard(AlgebraicCard::Mult),
            Card::BasisCard(BasisCard::Zero),
        ];
        let opponent_hand = vec![Card::BasisCard(BasisCard::X)];

        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4);
        assert!(
            candidates.iter().any(|mv| mv.wins_immediately),
            "test setup is wrong: multiplying field[3] by the hand's 0 should have \
             produced a winning candidate, but none was generated -- Mult/Div with a \
             hand BasisCard operand isn't being considered at all"
        );
        let chosen = choose_move(candidates, &ai_hand, &opponent_hand, AiDifficulty::Hard, 4, &field, std::time::Duration::from_millis(200));

        assert!(
            chosen.wins_immediately,
            "AI had a winning move (multiply opponent's last slot by 0 from hand) \
             available but chose a different one instead"
        );
    }

    /// priority tier 2: never leave the opponent an immediate winning reply when a
    /// safe alternative exists, even if the unsafe move's own immediate score looks
    /// more attractive
    #[test]
    fn test_ai_avoids_immediate_loss_when_a_safe_alternative_exists() {
        // Field::new() starts every slot occupied -- slots meant to be empty must
        // be cleared explicitly
        let mut field = Field::new();
        // AI's only remaining own slot: "1" -- the opponent's Derivative card would
        // zero this (an immediate win for them) unless the AI changes it first
        field[0] = FieldBasis::new(&Basis::from(1));
        field[1] = FieldBasis::none();
        field[2] = FieldBasis::none();
        // an unrelated opponent slot the AI could otherwise be tempted to target
        field[3] = FieldBasis::new(&Basis::from(BasisCard::X2));

        let ai_hand = vec![
            // safe: integral(1) = x, so the opponent's derivative no longer zeroes it
            Card::DerivativeCard(DerivativeCard::Integral),
            // unsafe if aimed at field[3]: simplifies the opponent's x^2 (tempting
            // score) but leaves field[0] untouched, so the threat on it survives
            Card::DerivativeCard(DerivativeCard::Derivative),
        ];
        let opponent_hand = vec![Card::DerivativeCard(DerivativeCard::Derivative)];

        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4);
        let chosen = choose_move(candidates, &ai_hand, &opponent_hand, AiDifficulty::Hard, 4, &field, std::time::Duration::from_millis(200));

        assert!(
            !has_winning_move(1, &opponent_hand, &chosen.resulting_field, 5),
            "AI chose a move that leaves the opponent an immediate winning reply, \
             despite a safe alternative (defusing field[0]) being available"
        );
    }

    /// the AI must never choose a move that instantly loses the game by its own
    /// hand (clearing its own last slot), even if that same move also simplifies an
    /// opponent slot enough to look net-positive by raw score
    #[test]
    fn test_ai_never_picks_a_self_defeating_move_when_avoidable() {
        // Field::new() starts every slot occupied -- slots meant to be empty must
        // be cleared explicitly
        let mut field = Field::new();
        // AI's only remaining own slot -- derivative(1) = 0 would clear it,
        // instantly losing the game by the AI's own hand
        field[0] = FieldBasis::new(&Basis::from(1));
        field[1] = FieldBasis::none();
        field[2] = FieldBasis::none();

        let ai_hand = vec![
            // self-defeating if aimed at field[0]: derivative(1) = 0 clears the
            // AI's own last slot, an instant loss regardless of its raw score
            Card::DerivativeCard(DerivativeCard::Derivative),
            // safe alternative: doesn't touch field[0]
            Card::AlgebraicCard(AlgebraicCard::Sqrt),
        ];
        let opponent_hand = vec![Card::BasisCard(BasisCard::X)];

        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4);
        assert!(
            candidates
                .iter()
                .any(|mv| side_is_cleared(&mv.resulting_field, AI_PLAYER_NUM)),
            "test setup is wrong: no self-defeating candidate was generated at all"
        );
        let chosen = choose_move(candidates, &ai_hand, &opponent_hand, AiDifficulty::Hard, 4, &field, std::time::Duration::from_millis(200));

        assert!(
            !side_is_cleared(&chosen.resulting_field, AI_PLAYER_NUM),
            "AI chose a move that instantly loses the game by clearing its own side"
        );
    }

    /// direct check of the rule hurts_self_or_helps_opponent uses: clearing any of
    /// the mover's own slots entirely, or growing any of the opponent's, should
    /// register as a violation regardless of which slot changed; merely shrinking
    /// (without clearing) one of the mover's own slots, growing your own, shrinking
    /// the opponent's, or leaving everything unchanged should not -- "attacking
    /// your own field" specifically means erasing its card, not simplifying it
    #[test]
    fn test_hurts_self_or_helps_opponent_detects_every_direction() {
        let mut field = Field::new();
        field[0] = FieldBasis::new(&Basis::from(1)); // AI's own slot (player 2), size 1
        field[3] = FieldBasis::new(&Basis::from(1)); // opponent's slot (player 1), size 1

        // clearing our own slot entirely is a violation
        let mut after = field.clone();
        after[0] = FieldBasis::none();
        assert!(hurts_self_or_helps_opponent(AI_PLAYER_NUM, &field, &after));

        // growing our own slot (X2 has size 2, see its BasisNode/Pow construction
        // in Basis::from(BasisCard)) is not a violation
        let mut after = field.clone();
        after[0] = FieldBasis::new(&Basis::from(BasisCard::X2));
        assert!(!hurts_self_or_helps_opponent(AI_PLAYER_NUM, &field, &after));

        // merely shrinking our own slot -- without clearing it -- is NOT a
        // violation: the loss condition only cares whether a slot is empty, not
        // how simple its expression is, so simplifying x^2 down to x (still
        // occupied) is not a step toward losing the way clearing it is
        let mut shrinking_field = field.clone();
        shrinking_field[0] = FieldBasis::new(&Basis::from(BasisCard::X2)); // size 2
        let mut after = shrinking_field.clone();
        after[0] = FieldBasis::new(&Basis::from(BasisCard::X)); // size 1, still occupied
        assert!(!hurts_self_or_helps_opponent(AI_PLAYER_NUM, &shrinking_field, &after));

        // shrinking the opponent's slot (clearing it entirely) is not a violation
        let mut after = field.clone();
        after[3] = FieldBasis::none();
        assert!(!hurts_self_or_helps_opponent(AI_PLAYER_NUM, &field, &after));

        // growing the opponent's slot is a violation
        let mut after = field.clone();
        after[3] = FieldBasis::new(&Basis::from(BasisCard::X2));
        assert!(hurts_self_or_helps_opponent(AI_PLAYER_NUM, &field, &after));

        // no change at all is not a violation
        assert!(!hurts_self_or_helps_opponent(AI_PLAYER_NUM, &field, &field));
    }

    /// the AI must never choose a move that attacks its own field (clears one of
    /// its own slots) whenever a safe alternative exists -- multiplying a field
    /// slot by a "0" from hand (see generate_candidates_for's field+hand-card
    /// Mult/Div pairing) naturally generates both directions as separate
    /// candidates from the same (target, hand_card) loop: clearing one of the
    /// AI's own slots (flagged) and clearing one of the opponent's (safe)
    #[test]
    fn test_ai_never_attacks_own_field_or_strengthens_opponent_field() {
        // Field::new() starts every slot occupied -- slots meant to be empty must
        // be cleared explicitly
        let mut field = Field::new();
        field[0] = FieldBasis::new(&Basis::from(1));
        // kept non-empty so losing field[0] isn't self-defeating
        field[1] = FieldBasis::new(&Basis::from(BasisCard::X));
        field[2] = FieldBasis::none();
        field[3] = FieldBasis::new(&Basis::from(BasisCard::X2));
        // kept non-empty so clearing field[3] doesn't win outright -- this test
        // targets tier 3, not tier 1
        field[4] = FieldBasis::new(&Basis::from(BasisCard::X));
        field[5] = FieldBasis::none();

        let ai_hand = vec![
            Card::AlgebraicCard(AlgebraicCard::Mult),
            Card::BasisCard(BasisCard::Zero),
        ];
        let opponent_hand = vec![Card::BasisCard(BasisCard::X)];

        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4);
        assert!(
            candidates.iter().any(|mv| mv.hurts_self_or_helps_opponent),
            "test setup is wrong: no self-attacking/opponent-strengthening candidate \
             was generated at all"
        );
        assert!(
            candidates.iter().any(|mv| !mv.hurts_self_or_helps_opponent),
            "test setup is wrong: no safe alternative candidate was generated at all"
        );
        let chosen = choose_move(candidates, &ai_hand, &opponent_hand, AiDifficulty::Hard, 4, &field, std::time::Duration::from_millis(200));

        assert!(
            !chosen.hurts_self_or_helps_opponent,
            "AI chose a move that attacks its own field or strengthens the \
             opponent's, despite a safe alternative being available"
        );
    }

    #[test]
    /// field+field Mult/Div (combining two field slots into one) is deprecated:
    /// multi_select_phase (events/mousedown_handler.rs) now accepts at most ONE
    /// field slot per Mult/Div play, silently dropping a second field click --
    /// so if generate_candidates_for ever produced a field+field candidate again,
    /// the AI would choose a move it could never actually complete (its second
    /// field click would be dropped, leaving the turn stranded one operand short
    /// of has_at_least_2_basis, with Multidone never triggering the commit).
    /// Every Mult/Div candidate must target exactly one field slot, paired with a
    /// BasisCard from hand
    fn test_mult_div_candidates_always_target_exactly_one_field_slot() {
        let mut field = Field::new();
        for i in 0..6 {
            field[i] = FieldBasis::new(&Basis::from(BasisCard::X));
        }

        for card in [
            Card::AlgebraicCard(AlgebraicCard::Mult),
            Card::AlgebraicCard(AlgebraicCard::Div),
        ] {
            let hand = vec![card, Card::BasisCard(BasisCard::Zero)];
            let candidates = generate_candidates_for(AI_PLAYER_NUM, &hand, &field, 4);
            assert!(
                !candidates.is_empty(),
                "test setup is wrong: no {card} candidates were generated at all"
            );
            for mv in &candidates {
                let field_indices: Vec<usize> = mv
                    .clicks
                    .iter()
                    .filter(|id| id.is_field())
                    .map(|id| id.key_val().1)
                    .collect();
                assert_eq!(
                    field_indices.len(),
                    1,
                    "{card} move targeted {field_indices:?} field slots, not exactly 1 -- \
                     field+field Mult/Div is deprecated and no longer executable"
                );
            }
        }
    }

    /// select_branch_candidates must never let a plain top-N-by-score truncation
    /// starve the search of every non-attacking (BUILD/REFRESH-style) candidate --
    /// see MINIMAX_MIN_NON_ATTACK_SLOTS's doc on why. Uses synthetic AiMove values
    /// directly (rather than a real field/hand) to precisely control how many of
    /// each kind exist and how they're scored, independent of anything
    /// generate_candidates_for happens to produce for a given position
    #[test]
    fn test_select_branch_candidates_reserves_non_attacking_slots() {
        let field = Field::new(); // opponent (player 1) slots 3-5 start at size-1/1/2

        let make_move = |score: f64, is_attack: bool| {
            let mut resulting_field = field.clone();
            if is_attack {
                // shrinks opponent's slot 3 (field_owner(3) == 1) from size 1 to 0
                resulting_field[3] = FieldBasis::none();
            } else {
                // grows the mover's own slot 0 instead -- never touches the
                // opponent's side at all
                resulting_field[0] = FieldBasis::new(&Basis::from(BasisCard::Sin));
            }
            AiMove {
                clicks: vec![],
                score,
                resulting_field,
                wins_immediately: false,
                hurts_self_or_helps_opponent: false,
            }
        };

        // far more attacking candidates than MINIMAX_BRANCH_CAP, every one of them
        // scored higher than every non-attacking candidate -- a plain top-N
        // truncation would keep zero non-attacking candidates
        let mut candidates: Vec<AiMove> = (0..MINIMAX_BRANCH_CAP * 2)
            .map(|i| make_move(1000.0 + i as f64, true))
            .collect();
        candidates.extend((0..MINIMAX_MIN_NON_ATTACK_SLOTS + 2).map(|i| make_move(i as f64, false)));

        let selected = select_branch_candidates(candidates, &field, AI_PLAYER_NUM);

        assert_eq!(selected.len(), MINIMAX_BRANCH_CAP, "should still fill the full cap");
        let non_attacking_kept = selected
            .iter()
            .filter(|mv| !is_attacking_move(mv, &field, AI_PLAYER_NUM))
            .count();
        assert_eq!(
            non_attacking_kept, MINIMAX_MIN_NON_ATTACK_SLOTS,
            "expected exactly the reserved number of non-attacking candidates to survive, \
             got {non_attacking_kept}"
        );
    }

    /// broad randomized search for the "AI attacks its own field" report: across
    /// many random (field, hand) combinations, if the AI's chosen move hurts
    /// itself/helps the opponent, was there an alternative that satisfied tier 0
    /// (not self-defeating), tier 2 (doesn't hand the opponent an immediate win
    /// next turn) AND tier 3 (doesn't hurt self/help opponent) all at once? If so,
    /// choose_move picked a strictly worse move than one that was available --
    /// a real bug, not just tier 2/tier 0 correctly outranking tier 3 when every
    /// tier-3-safe candidate was already eliminated by a higher-priority tier.
    /// This property is guaranteed by apply_hard_filters before Hard's minimax
    /// search (or Medium/Easy's own ranking) ever runs, so it holds regardless of
    /// search depth -- each difficulty gets its own trial count below sized to how
    /// expensive a single choose_move call is at that difficulty, not to how
    /// interesting a difficulty is to test; Hard's real minimax search is
    /// meaningfully more expensive per call than a heuristic ranking even at a
    /// tiny time budget, so it gets fewer trials to keep this test's own runtime
    /// reasonable
    #[test]
    fn test_ai_never_hurts_itself_when_a_fully_safe_alternative_exists() {
        let basis_pool: Vec<Basis> = vec![
            Basis::from(1),
            Basis::from(BasisCard::X),
            Basis::from(BasisCard::X2),
            Basis::from(BasisCard::Sin),
            Basis::from(BasisCard::Cos),
            Basis::from(BasisCard::E),
            derivative(&Basis::from(BasisCard::X2)),
            crate::math::integral::integral(&Basis::from(BasisCard::X2)),
            crate::math::integral::integral(&crate::math::integral::integral(&Basis::from(
                BasisCard::X,
            ))),
        ];
        let card_pool: Vec<Card> = vec![
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

        let mut rng = rand::thread_rng();
        let mut violations: Vec<String> = vec![];
        let mut checked = 0;

        for trial in 0..1500 {
            let mut field = Field::new();
            for i in 0..6 {
                if rng.gen_bool(0.2) {
                    field[i] = FieldBasis::none();
                } else {
                    let basis = basis_pool[rng.gen_range(0..basis_pool.len())].clone();
                    field[i] = FieldBasis::new(&basis);
                }
            }
            // skip fields where someone has already won -- not a real decision point
            if side_is_cleared(&field, 1) || side_is_cleared(&field, 2) {
                continue;
            }

            let hand_size = rng.gen_range(3..=7);
            let ai_hand: Vec<Card> = (0..hand_size)
                .map(|_| card_pool[rng.gen_range(0..card_pool.len())].clone())
                .collect();
            let opponent_hand: Vec<Card> = (0..hand_size)
                .map(|_| card_pool[rng.gen_range(0..card_pool.len())].clone())
                .collect();
            let turn_number = rng.gen_range(2..20); // >=2 so Laplacian is in play

            let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, turn_number);
            if candidates.is_empty() {
                continue;
            }
            checked += 1;

            // Hard's real minimax search costs meaningfully more per call than
            // Easy/Medium's heuristic ranking even at a tiny time budget (it still
            // has to actually run alpha-beta over however much of the tree that
            // budget allows) -- only every 10th trial exercises it, so this test's
            // own runtime stays reasonable without losing meaningful coverage of a
            // property that Hard's minimax has no special ability to violate in
            // the first place (see this test's own doc)
            let difficulties: &[AiDifficulty] = if trial % 10 == 0 {
                &[AiDifficulty::Easy, AiDifficulty::Medium, AiDifficulty::Hard]
            } else {
                &[AiDifficulty::Easy, AiDifficulty::Medium]
            };
            for &difficulty in difficulties {
                let candidates_for_difficulty =
                    generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, turn_number);
                // a tiny budget here, not DEFAULT_MINIMAX_TIME_BUDGET's real 5s --
                // this property (never hurt self when a fully-safe alternative
                // exists) is guaranteed by apply_hard_filters before minimax ever
                // runs, so it holds regardless of how deep the search gets;
                // running thousands of trials at 5 real seconds each would make
                // this test itself unusably slow for no corresponding benefit
                let chosen = choose_move(
                    candidates_for_difficulty,
                    &ai_hand,
                    &opponent_hand,
                    difficulty,
                    turn_number,
                    &field,
                    std::time::Duration::from_millis(20),
                );
                if !chosen.hurts_self_or_helps_opponent {
                    continue;
                }
                let better_exists = candidates.iter().any(|mv| {
                    !mv.hurts_self_or_helps_opponent
                        && !side_is_cleared(&mv.resulting_field, AI_PLAYER_NUM)
                        && !has_winning_move(1, &opponent_hand, &mv.resulting_field, turn_number + 1)
                });
                if better_exists {
                    violations.push(format!(
                        "trial={trial} difficulty={difficulty:?}: chosen hurt self/helped \
                         opponent despite a fully-safe alternative existing -- field={:?} \
                         ai_hand={:?} chosen.clicks={:?}",
                        field, ai_hand, chosen.clicks
                    ));
                }
            }
        }

        println!("checked {checked} trials with at least one candidate");
        for v in &violations {
            println!("{v}");
        }
        assert!(
            violations.is_empty(),
            "found {} case(s) where the AI chose a self-harming move despite a fully-safe \
             alternative -- see stdout for details",
            violations.len()
        );
    }

    /// mirrors handle_derivative_card's (events/mousedown_handler.rs) exact shortcut
    /// logic: if the target index was visited before, reuse the cached history
    /// value instead of recomputing. Used to check whether that cached value ever
    /// diverges from what generate_candidates_for's simulation would have
    /// predicted for the same move (it always computes fresh via apply_card,
    /// never consulting history) -- see the doc on
    /// test_derivative_integral_round_trip_matches_a_fresh_computation
    fn apply_derivative_card_like_real_execution(field: &mut Field, i: usize, card: Card) {
        let is_laplacian = matches!(card, Card::DerivativeCard(DerivativeCard::Laplacian));
        let is_integral = matches!(card, Card::DerivativeCard(DerivativeCard::Integral));
        let is_derivative = matches!(
            card,
            Card::DerivativeCard(DerivativeCard::Derivative | DerivativeCard::Nabla)
        );
        if field[i].has_value(&card) {
            if is_derivative || is_laplacian {
                field.derivative(i, None);
            } else if is_integral {
                field.integral(i, None);
            }
            if is_laplacian {
                field.derivative(i, None);
            }
        } else {
            let result_basis = apply_card(&card)(field[i].basis.as_ref().unwrap());
            if is_derivative || is_laplacian {
                field.derivative(i, Some(result_basis.clone()));
            } else if is_integral {
                field.integral(i, Some(result_basis.clone()));
            }
            if is_laplacian {
                let second = apply_card(&card)(&result_basis);
                field.derivative(i, Some(second));
            }
        }
    }

    /// the AI's own candidate generation (generate_candidates_for) always scores a
    /// Derivative/Integral/Nabla play by computing apply_card(&card)(CURRENT basis)
    /// fresh -- it never consults FieldBasis::history. But the REAL execution path
    /// (handle_derivative_card) takes a shortcut whenever the target index was
    /// visited before: instead of recomputing, it jumps straight to the cached
    /// value from that earlier visit (see Field::derivative/integral). Those two
    /// can only ever agree if this CAS's derivative and integral are exact,
    /// consistent inverses of each other for every expression the game can
    /// produce -- if they're not (eg. a normalization difference, or the "Not yet
    /// implemented" integration fallback in math/integral.rs leaving an
    /// unsimplified wrapper node), the AI would score a move against one value
    /// while the real game applies a completely different one, which could easily
    /// look like "the AI attacked its own field" even though hurts_self_or_helps_opponent
    /// correctly evaluated the (wrong) predicted outcome
    #[test]
    fn test_derivative_integral_round_trip_matches_a_fresh_computation() {
        let starting_bases = vec![
            Basis::from(1),
            Basis::from(BasisCard::X),
            Basis::from(BasisCard::X2),
            Basis::from(BasisCard::Sin),
            Basis::from(BasisCard::Cos),
            Basis::from(BasisCard::E),
            // Sqrt introduces a Pow(1/2) node -- integrating one of these enough
            // times is what actually hit integral.rs's "Not yet implemented"
            // fallback (an unsimplified IntBasisNode wrapper) during the earlier
            // randomized search, so it gets its own round-trip check here
            SqrtBasisNode(1, &Basis::from(BasisCard::X)),
            SqrtBasisNode(1, &Basis::from(BasisCard::X2)),
        ];

        let mut mismatches: Vec<String> = vec![];

        for start in &starting_bases {
            // integrate up several levels (as a real multi-turn game would),
            // building real history via the same path handle_derivative_card uses.
            // 6 levels (deeper than the 4 used for the simpler bases above) to
            // reliably reach the LIATE fallback for the sqrt-based starting bases
            let mut field = Field::new();
            field[0] = FieldBasis::new(start);
            for _level in 0..6 {
                apply_derivative_card_like_real_execution(
                    &mut field,
                    0,
                    Card::DerivativeCard(DerivativeCard::Integral),
                );
            }

            // now come back down: at each step, compare what the real shortcut
            // produces against a fresh derivative computed directly from the
            // basis the AI would have seen (ie. what it actually scored)
            for _level in 0..6 {
                let basis_before = field[0].basis.clone().unwrap();
                let predicted_by_ai = derivative(&basis_before);

                apply_derivative_card_like_real_execution(
                    &mut field,
                    0,
                    Card::DerivativeCard(DerivativeCard::Derivative),
                );
                let actual_after_real_execution = field[0].basis.clone().unwrap();

                if predicted_by_ai != actual_after_real_execution {
                    mismatches.push(format!(
                        "start={start:?}: from {basis_before:?}, AI would have predicted \
                         {predicted_by_ai:?} but real execution (history shortcut) produced \
                         {actual_after_real_execution:?}"
                    ));
                }
            }
        }

        for m in &mismatches {
            println!("{m}");
        }
        assert!(
            mismatches.is_empty(),
            "found {} case(s) where the history shortcut disagrees with a fresh computation \
             -- see stdout for details",
            mismatches.len()
        );
    }

}
