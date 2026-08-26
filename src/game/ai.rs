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
    generate_candidates_for(player_num, hand, field, turn_number, true)
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
    // true: the AI's own real candidates must include Mult/Div, or it would never
    // consider (or be able to play) those cards at all
    let candidates = generate_candidates_for(
        AI_PLAYER_NUM,
        &game.player_2,
        &game.field,
        game.turn.number,
        true,
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
    let chosen = choose_move(candidates, &game.player_1, difficulty, game.turn.number, &game.field);
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
/// ever shrinks one of `player_num`'s own slots, or grows one of the opponent's --
/// the two things this game's actual objective (empty the opponent's side, keep
/// your own non-empty) always makes counter-productive, regardless of which card
/// produced them. Checks every field slot, so this catches every move type
/// uniformly -- single-target operators, Nabla/Laplacian's half-field derivative,
/// Mult/Div's two-slot combine (both the result slot and the sacrificed one), and
/// BasisCard placement into an empty slot (size 0 -> positive counts as "growing"
/// that slot, so filling the opponent's empty slot is caught here too) -- without
/// needing a special case wired into each one individually. A move that merely
/// leaves a slot's size unchanged (eg. Integral(1) = x, both size 1) is neutral,
/// not a violation either way
fn hurts_self_or_helps_opponent(player_num: u32, field: &Field, resulting_field: &Field) -> bool {
    (0..6).any(|i| {
        let old_size = field[i].basis.as_ref().map(basis_size).unwrap_or(0);
        let new_size = resulting_field[i].basis.as_ref().map(basis_size).unwrap_or(0);
        if field_owner(i) == player_num {
            new_size < old_size
        } else {
            new_size > old_size
        }
    })
}

/// scores replacing `field[target]` with `new_basis`, from `evaluating_player`'s
/// point of view: rewards clearing/simplifying the OTHER player's slot (progress
/// toward winning), penalises clearing evaluating_player's own slot. Reused both to
/// score the AI's own candidates (evaluating_player = AI_PLAYER_NUM) and to predict
/// the human's best reply one ply ahead (evaluating_player = 1) -- see apply_lookahead
/// Enhanced with strategic scoring to prioritize optimal play
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
    
    // Strategic bonus: count how many opponent slots remain after this move
    let opponent_slots_remaining: u32 = (0..6)
        .filter(|&i| field_owner(i) != evaluating_player)
        .filter(|&i| {
            if i == target {
                new_size > 0
            } else {
                field[i].basis.is_some()
            }
        })
        .count() as u32;
    
    // Bonus for reducing opponent's remaining slots (closer to victory)
    score += (3.0 - opponent_slots_remaining as f64) * 15.0;
    
    // Penalty for reducing own remaining slots (closer to defeat)
    let own_slots_remaining: u32 = (0..6)
        .filter(|&i| field_owner(i) == evaluating_player)
        .filter(|&i| {
            if i == target {
                new_size > 0
            } else {
                field[i].basis.is_some()
            }
        })
        .count() as u32;
    
    score -= (3.0 - own_slots_remaining as f64) * 25.0;
    
    score
}

/// scores a Nabla/Laplacian play across all 3 slots of the targeted half, from
/// `evaluating_player`'s point of view (see score_replacement). Whether this ends
/// up favouring or hurting `evaluating_player` is left entirely to score_replacement
/// -- applying it to their own half isn't hard-excluded here since a derivative can
/// occasionally *grow* an expression (eg. product rule), which the general
/// hurts_self_or_helps_opponent filter (based on actual resulting sizes, not which
/// half was targeted) already lets through as a legitimate defensive play
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
/// combinations) to keep the search small; every other card type is fully covered.
/// `include_multiselect` gates the Mult/Div branch entirely -- it's the single most
/// expensive part of this search (up to 30 candidates, each building an actual
/// symbolic Basis via apply_multi_card), and the *opponent's* simulated reply
/// (see best_reply_score) only needs a cheap, good-enough danger estimate, not a
/// fully exhaustive one -- doing a full search on both the AI's own candidates AND
/// every one of their simulated opponent replies is what made deep lookahead slow
/// enough to look like the AI had frozen
fn generate_candidates_for(
    player_num: u32,
    hand: &[Card],
    field: &Field,
    turn_number: u32,
    include_multiselect: bool,
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
                        let wins_immediately =
                            side_is_cleared(&resulting_field, opponent_of(player_num));
                        let hurts_self_or_helps_opponent =
                            hurts_self_or_helps_opponent(player_num, field, &resulting_field);
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={target}"))],
                            score: score_replacement(player_num, target, &new_basis, field),
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
                        score: score_half(player_num, half_start, false, field),
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
                        score: score_half(player_num, half_start, true, field),
                        resulting_field,
                        wins_immediately,
                        hurts_self_or_helps_opponent,
                    });
                }
            }
            Card::AlgebraicCard(AlgebraicCard::Div | AlgebraicCard::Mult) if include_multiselect => {
                for a in 0..6 {
                    for b in 0..6 {
                        // Mult/Div combines two of the SAME player's own slots (build up
                        // your own side) or two of the opponent's (simplify their side
                        // using their own expressions) -- never one from each. Without
                        // this, `a` (kept, merged) and `b` (always sacrificed, see below)
                        // could land on opposite sides, so a single card play would
                        // simultaneously alter both players' fields in one move: grow
                        // whichever side `a` is on while emptying `b`'s slot on the
                        // other side entirely -- a visibly different (and much stronger)
                        // effect than every other card, which only ever touches one
                        // player's side per play
                        if a == b
                            || field[a].basis.is_none()
                            || field[b].basis.is_none()
                            || field_owner(a) != field_owner(b)
                        {
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
                        let wins_immediately =
                            side_is_cleared(&resulting_field, opponent_of(player_num));
                        let hurts_self_or_helps_opponent =
                            hurts_self_or_helps_opponent(player_num, field, &resulting_field);
                        moves.push(AiMove {
                            clicks: vec![
                                hand_id,
                                RenderId::from(format!("f={a}")),
                                RenderId::from(format!("f={b}")),
                                RenderId::Multidone,
                            ],
                            score,
                            resulting_field,
                            wins_immediately,
                            hurts_self_or_helps_opponent,
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
                        let wins_immediately =
                            side_is_cleared(&resulting_field, opponent_of(player_num));
                        let hurts_self_or_helps_opponent =
                            hurts_self_or_helps_opponent(player_num, field, &resulting_field);
                        moves.push(AiMove {
                            clicks: vec![hand_id, RenderId::from(format!("f={target}"))],
                            score: score_replacement(player_num, target, &result, field),
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
/// handling for the AI's symmetric case). Always searched without Mult/Div (see
/// generate_candidates_for's include_multiselect doc) -- this is a danger *estimate*,
/// not the opponent's real move, so it doesn't need to be exhaustive
fn best_reply_score(player_num: u32, hand: &[Card], field: &Field, turn_number: u32) -> f64 {
    generate_candidates_for(player_num, hand, field, turn_number, false)
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

/// difficulty-scaled selection, applying priority tiers roughly highest-to-lowest:
/// (1) an immediate win is always taken outright, for every difficulty -- see
/// wins_immediately; (2) an immediate loss is always avoided if any alternative
/// exists, for every difficulty -- see filter_out_losing_moves; (3) the AI never
/// attacks its own field or strengthens the opponent's if there's any alternative,
/// for every difficulty -- see filter_out_moves_that_hurt_self_or_help_opponent;
/// (4) always make progress toward winning if possible; (5) never directly
/// strengthen the opponent if there's an alternative; (6+) among what's left, Hard
/// looks one ply ahead across *every* remaining candidate (see
/// apply_lookahead's doc on why a pool isn't safe here) and always takes the
/// best-adjusted move, Medium looks ahead too but only across its top
/// MEDIUM_LOOKAHEAD_POOL candidates and picks randomly from a shrinking top slice of
/// that adjusted ranking (reasonably aware, not required to be perfect), and Easy
/// skips lookahead entirely and stays mostly random so it remains reliably beatable
fn choose_move(
    candidates: Vec<AiMove>,
    opponent_hand: &[Card],
    difficulty: AiDifficulty,
    turn_number: u32,
    field: &Field,
) -> AiMove {
    // tier 0 (implicit, checked first): never choose a move that instantly ends the
    // game in the AI's own defeat, if any alternative exists
    let candidates = filter_out_self_defeating_moves(candidates);

    // tier 1: never pass up a move that wins outright, regardless of difficulty
    if let Some(winning_index) = candidates.iter().position(|mv| mv.wins_immediately) {
        let mut candidates = candidates;
        return candidates.remove(winning_index);
    }

    // tier 2: never hand the opponent a win next turn if there's any alternative
    let candidates = filter_out_losing_moves(candidates, opponent_hand, turn_number);

    // tier 3: never attack our own field or strengthen the opponent's, if there's
    // any alternative -- see filter_out_moves_that_hurt_self_or_help_opponent
    let candidates = filter_out_moves_that_hurt_self_or_help_opponent(candidates);
    
    // tier 4: always make progress toward winning if possible
    let candidates = filter_out_non_progressive_moves(candidates, field);

    // tier 5: never directly strengthen the opponent if there's an alternative
    let candidates = filter_out_opponent_strengthening_moves(candidates, field);

    let mut candidates = match difficulty {
        AiDifficulty::Hard => apply_lookahead(candidates, opponent_hand, turn_number, usize::MAX),
        AiDifficulty::Medium => {
            apply_lookahead(candidates, opponent_hand, turn_number, MEDIUM_LOOKAHEAD_POOL)
        }
        AiDifficulty::Easy => candidates,
    };
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    let mut rng = rand::thread_rng();

    let index = match difficulty {
        AiDifficulty::Hard => 0, // Always take the best move on Hard
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
    };
    candidates.remove(index)
}

#[cfg(test)]
mod perf_tests {
    use super::*;
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

    /// stacks the hand with the single most expensive card type (Mult/Div, up to 30
    /// pair-combinations each) to stress the search as hard as realistically possible
    fn worst_case_hand() -> Vec<Card> {
        vec![
            Card::AlgebraicCard(AlgebraicCard::Mult),
            Card::AlgebraicCard(AlgebraicCard::Div),
            Card::AlgebraicCard(AlgebraicCard::Mult),
            Card::AlgebraicCard(AlgebraicCard::Div),
            Card::DerivativeCard(DerivativeCard::Derivative),
            Card::DerivativeCard(DerivativeCard::Integral),
            Card::AlgebraicCard(AlgebraicCard::Sqrt),
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
    /// complete quickly and without panicking against a field shaped like a real,
    /// long-running game rather than a freshly-dealt one
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

        let start = std::time::Instant::now();
        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 20, true);
        assert!(!candidates.is_empty(), "long-game field should still have legal moves");
        let _chosen = choose_move(candidates, &opponent_hand, AiDifficulty::Hard, 20, &field);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 3000,
            "AI decision against a long-game field took {:?} -- too slow, risks looking frozen",
            elapsed
        );
    }

    /// reproduces the "AI stops" report: with the pool cap removed for Hard (so it
    /// checks every one of its own candidates) and both the AI's and opponent's hands
    /// stacked with the most expensive card type in both slots of the lookahead, does
    /// a single decision still complete in a time that reads as responsive rather
    /// than frozen? This is the actual worst case the real game can produce (Mult/Div
    /// is the single most expensive branch; nothing is more expensive than having it
    /// on both sides of the nested search)
    #[test]
    fn test_hard_ai_decision_completes_quickly_in_worst_case() {
        let field = full_field();
        let ai_hand = worst_case_hand();
        let opponent_hand = worst_case_hand();

        let start = std::time::Instant::now();
        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4, true);
        assert!(!candidates.is_empty(), "worst-case hand should still have legal moves");
        let candidate_count = candidates.len();
        let chosen = choose_move(candidates, &opponent_hand, AiDifficulty::Hard, 4, &field);
        let elapsed = start.elapsed();

        println!(
            "worst-case Hard AI: {} candidates, decided in {:?}, chose {} clicks",
            candidate_count,
            elapsed,
            chosen.clicks.len()
        );
        assert!(
            elapsed.as_millis() < 3000,
            "AI decision took {:?} against a worst-case hand -- too slow, risks looking frozen",
            elapsed
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

        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4, true);
        assert!(
            candidates.iter().any(|mv| mv.wins_immediately),
            "test setup is wrong: no winning candidate was generated at all"
        );
        let chosen = choose_move(candidates, &opponent_hand, AiDifficulty::Hard, 4, &field);

        assert!(
            chosen.wins_immediately,
            "AI had a winning move available but chose a different one instead"
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

        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4, true);
        let chosen = choose_move(candidates, &opponent_hand, AiDifficulty::Hard, 4, &field);

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

        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4, true);
        assert!(
            candidates
                .iter()
                .any(|mv| side_is_cleared(&mv.resulting_field, AI_PLAYER_NUM)),
            "test setup is wrong: no self-defeating candidate was generated at all"
        );
        let chosen = choose_move(candidates, &opponent_hand, AiDifficulty::Hard, 4, &field);

        assert!(
            !side_is_cleared(&chosen.resulting_field, AI_PLAYER_NUM),
            "AI chose a move that instantly loses the game by clearing its own side"
        );
    }

    /// direct check of the size-comparison rule hurts_self_or_helps_opponent uses:
    /// shrinking any of the mover's own slots, or growing any of the opponent's,
    /// should register as a violation regardless of which slot changed; growing
    /// your own, shrinking the opponent's, or leaving everything unchanged should not
    #[test]
    fn test_hurts_self_or_helps_opponent_detects_every_direction() {
        let mut field = Field::new();
        field[0] = FieldBasis::new(&Basis::from(1)); // AI's own slot (player 2), size 1
        field[3] = FieldBasis::new(&Basis::from(1)); // opponent's slot (player 1), size 1

        // shrinking our own slot (clearing it entirely) is a violation
        let mut after = field.clone();
        after[0] = FieldBasis::none();
        assert!(hurts_self_or_helps_opponent(AI_PLAYER_NUM, &field, &after));

        // growing our own slot (X2 has size 2, see its BasisNode/Pow construction
        // in Basis::from(BasisCard)) is not a violation
        let mut after = field.clone();
        after[0] = FieldBasis::new(&Basis::from(BasisCard::X2));
        assert!(!hurts_self_or_helps_opponent(AI_PLAYER_NUM, &field, &after));

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

    /// the AI must never choose a move that attacks its own field (shrinks one of
    /// its own slots) or strengthens the opponent's field (grows one of theirs),
    /// whenever a safe alternative exists -- a single Mult/Div card naturally
    /// generates both directions (which slot it sacrifices, which one it grows) as
    /// separate candidates from the same (a, b) loop in generate_candidates_for, so
    /// this only requires giving the AI that one card
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

        let ai_hand = vec![Card::AlgebraicCard(AlgebraicCard::Mult)];
        let opponent_hand = vec![Card::BasisCard(BasisCard::X)];

        let candidates = generate_candidates_for(AI_PLAYER_NUM, &ai_hand, &field, 4, true);
        assert!(
            candidates.iter().any(|mv| mv.hurts_self_or_helps_opponent),
            "test setup is wrong: no self-attacking/opponent-strengthening candidate \
             was generated at all"
        );
        assert!(
            candidates.iter().any(|mv| !mv.hurts_self_or_helps_opponent),
            "test setup is wrong: no safe alternative candidate was generated at all"
        );
        let chosen = choose_move(candidates, &opponent_hand, AiDifficulty::Hard, 4, &field);

        assert!(
            !chosen.hurts_self_or_helps_opponent,
            "AI chose a move that attacks its own field or strengthens the \
             opponent's, despite a safe alternative being available"
        );
    }

    #[test]
    /// Mult/Div combine two field slots into one, always keeping `a` (merged
    /// result) and discarding `b` -- if `a` and `b` could land on opposite
    /// sides, a single card play would alter both players' fields at once
    /// (grow whichever side `a` is on, empty `b`'s slot on the other side),
    /// unlike every other card, which only ever touches one player's side per
    /// play. Every field fully occupied maximizes the number of Mult/Div
    /// candidates generated, giving the cross-side check the most surface
    /// area to catch a regression on
    fn test_mult_div_candidates_never_combine_slots_from_both_sides() {
        let mut field = Field::new();
        for i in 0..6 {
            field[i] = FieldBasis::new(&Basis::from(BasisCard::X));
        }

        for card in [
            Card::AlgebraicCard(AlgebraicCard::Mult),
            Card::AlgebraicCard(AlgebraicCard::Div),
        ] {
            let hand = vec![card];
            let candidates = generate_candidates_for(AI_PLAYER_NUM, &hand, &field, 4, true);
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
                    2,
                    "{card} move didn't target exactly 2 field slots: {field_indices:?}"
                );
                assert_eq!(
                    field_owner(field_indices[0]),
                    field_owner(field_indices[1]),
                    "{card} combined slots {field_indices:?} from both sides of the field \
                     in a single move"
                );
            }
        }
    }
}
