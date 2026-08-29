// outer crate imports
use crate::basis::structs::*;
use crate::game::cards::*;
use crate::game::{
    field::{Field, FieldBasis},
    flags::{ALLOW_LINEAR_DEPENDENCE, CONFIRM_BEFORE_PLAY},
    structs::*,
};
use crate::render::anim;
use crate::render::render;
use crate::render::util::RenderId;
// root imports
use crate::{CANVAS, GAME};

/// delegates event handling based on turn num
pub fn handle_mousedown(str_id: String) {
    let game = unsafe { GAME.as_mut().unwrap() };
    let turn = &game.turn;
    let id = RenderId::from(str_id);

    // tapping a field card with an expression toggles it between clipped and fully
    // shown, independent of whatever game action the tap also triggers below
    if id.is_field() && game.field[id.key_val().1].basis.is_some() {
        let canvas = unsafe { CANVAS.as_mut().unwrap() };
        if !canvas.expanded_cards.remove(&id) {
            canvas.expanded_cards.insert(id);
        }
        render::draw(); // reflect the toggle even if the tap otherwise triggers no game action
    }

    // in a PLAYAI game, the AI's turn belongs to the AI alone -- without this, a click
    // on the canvas during the AI's turn is dispatched exactly like a real player 2
    // move (turn phase routing only looks at whose turn number it is, not who is
    // actually meant to be playing it), letting a human take over mid-game
    if matches!(game.state, GameState::PLAYAI)
        && game.get_current_player_num() == crate::game::ai::AI_PLAYER_NUM
    {
        return;
    }

    // in a PLAYONLINE game, the local browser may only act on its own turns --
    // the remote player's turns are driven exclusively by replaying messages
    // received over the network (see on_action_received in game/online.rs)
    if matches!(game.state, GameState::PLAYONLINE) {
        let local_player_num = unsafe { crate::game::online::ONLINE_SESSION.as_ref() }
            .map(|session| session.local_player_num);
        if local_player_num != Some(game.get_current_player_num()) {
            return;
        }
    }

    // record this click for relay if it's part of the local player's own
    // online turn (the guard above already filtered out anything else)
    if matches!(game.state, GameState::PLAYONLINE) {
        if let Some(session) = unsafe { crate::game::online::ONLINE_SESSION.as_mut() } {
            session.record_click(id);
        }
    }

    match turn {
        Turn { number: n, .. } if n % 2 == 0 => {
            // even-number turn, player 1
            branch_turn_phase(id, 1);
        }
        Turn { number: n, .. } if n % 2 == 1 => {
            // odd-number turn, player 2
            branch_turn_phase(id, 2);
        }
        _ => unreachable!("Turn Number is not even or odd?"),
    }
}

/// further splits click event based on turn phase
pub fn branch_turn_phase(id: RenderId, player_num: u32) {
    let game = unsafe { GAME.as_mut().unwrap() };

    // every click, human or AI, passes through here (the AI's own clicks are
    // dispatched directly to this function, bypassing handle_mousedown -- see
    // try_take_ai_turn), so this is the one place that sees both sides of a
    // PLAYAI match uniformly for the "Copy Match Data" export
    if matches!(game.state, GameState::PLAYAI) {
        crate::game::match_log::record_click(id);
    }

    let turn = &game.turn;
    let player = if player_num == 1 {
        &game.player_1
    } else {
        &game.player_2
    };

    let (id_key, id_val) = id.key_val();

    // cancel button: also discards any move awaiting confirmation
    if id_key == "x" && id_val == 0 {
        game.active.clear();
        game.pending = None;
        next_phase(TurnPhase::IDLE);
        return;
    }

    // confirm button: commits the previewed move
    if id_key == "x" && id_val == 2 && matches!(turn.phase, TurnPhase::CONFIRM) {
        if let Some(pending) = game.pending.take() {
            game.field = pending.field;
            end_turn();
        }
        return;
    }

    match turn.phase {
        TurnPhase::IDLE if id_key == format!("p{}", player_num) => {
            game.active.selected.push(id);
            idle_turn_phase(player[id_val]);
        }
        TurnPhase::SELECT(select_operator) => select_turn_phase(select_operator, (id_key, id_val)),
        TurnPhase::FIELD_SELECT(field_operator) if id_key == "f" => {
            field_select_phase(field_operator, (id_key, id_val))
        }
        TurnPhase::MULTISELECT(multi_operator) => {
            multi_select_phase(multi_operator, id, player_num)
        }
        _ => {} // js_log!("Turn Phase Error: received {} on turn {:?}", id, turn),
    }
}

/// either commits `new_field` immediately and ends the turn, or -- when
/// CONFIRM_BEFORE_PLAY is on -- stores it as a preview and waits for the player to
/// confirm or cancel via the Confirm/Cancel buttons before it takes effect
fn commit_or_confirm(new_field: Field, changed_indices: Vec<usize>) {
    let game = unsafe { GAME.as_mut().unwrap() };
    let flag = unsafe { CONFIRM_BEFORE_PLAY };
    if flag {
        game.pending = Some(PendingAction {
            field: new_field,
            changed_indices,
        });
        next_phase(TurnPhase::CONFIRM);
    } else {
        game.field = new_field;
        end_turn();
    }
}

/// handles idle turn phase, where player can select a card
fn idle_turn_phase(card: Card) {
    let game = unsafe { GAME.as_mut().unwrap() };

    // match against current card in player hand
    match card {
        Card::BasisCard(basis_card) => {
            // allow play if empty slot
            if game.field.basis.iter().any(|b| b.basis.is_none()) {
                next_phase(TurnPhase::SELECT(Card::BasisCard(basis_card)));
            }
        }
        Card::DerivativeCard(derivative_card) => {
            if matches!(
                derivative_card,
                DerivativeCard::Derivative | DerivativeCard::Integral
            ) {
                next_phase(TurnPhase::SELECT(card));
            } else if matches!(derivative_card, DerivativeCard::Nabla)
            // prevent player from playing laplacian on first turn for each player
                || (game.turn.number >= 2 && matches!(derivative_card, DerivativeCard::Laplacian))
            {
                // field select
                next_phase(TurnPhase::FIELD_SELECT(Card::DerivativeCard(
                    derivative_card,
                )));
            }
        }
        // multiselect
        Card::AlgebraicCard(algebraic_card)
            if matches!(algebraic_card, AlgebraicCard::Div | AlgebraicCard::Mult) =>
        {
            next_phase(TurnPhase::MULTISELECT(Card::AlgebraicCard(algebraic_card)));
        }
        // select
        card => {
            next_phase(TurnPhase::SELECT(card));
        }
    }
}

/// handles select turn phase, player can choose single target of selected card
fn select_turn_phase(select_operator: Card, (id_key, id_val): (String, usize)) {
    let game = unsafe { GAME.as_mut().unwrap() };

    match select_operator {
        // play basis from hand if empty slot
        Card::BasisCard(basis_card) => {
            if id_key == "f"
                && game.field[id_val].basis.is_none()
                && !matches!(basis_card, BasisCard::Zero)
            {
                let mut new_field = game.field.clone();
                new_field[id_val] = FieldBasis::new(&Basis::from(basis_card));
                commit_or_confirm(new_field, vec![id_val]);
            }
        }
        // play function from hand onto field
        operator_card => {
            if id_key == "f" {
                let mut new_field = game.field.clone();
                if matches!(
                    operator_card,
                    Card::DerivativeCard(DerivativeCard::Derivative | DerivativeCard::Integral)
                ) {
                    handle_derivative_card(&mut new_field, operator_card, id_val);
                } else if matches!(operator_card, Card::AlgebraicCard(AlgebraicCard::Inverse)) {
                    let result_basis =
                        apply_card(&operator_card)(new_field[id_val].basis.as_ref().unwrap());
                    new_field.inverse(id_val, Some(result_basis))
                } else {
                    let result_basis =
                        apply_card(&operator_card)(new_field[id_val].basis.as_ref().unwrap());
                    if result_basis.is_num(0) || result_basis.is_inf(1) || result_basis.is_inf(-1) {
                        new_field[id_val] = FieldBasis::none();
                    } else {
                        new_field[id_val] = FieldBasis::new(&result_basis);
                    }
                }
                commit_or_confirm(new_field, vec![id_val]);
            }
        }
    }
}

/// handles field select turn phase, player can choose side of field to target with selected card
fn field_select_phase(field_operator: Card, (_id_key, id_val): (String, usize)) {
    let game = unsafe { GAME.as_mut().unwrap() };
    let card_range = if id_val < 3 { 0..3 } else { 3..6 };
    let mut new_field = game.field.clone();
    let changed_indices: Vec<usize> = card_range.clone().collect();
    // for each basis on one half of the field
    for i in card_range {
        handle_derivative_card(&mut new_field, field_operator, i);
    }
    commit_or_confirm(new_field, changed_indices);
}

/// manages derivatives of FieldBasis, looks up history of derivatives/integrals and applies if possible
fn handle_derivative_card(field: &mut Field, card: Card, i: usize) {
    let is_laplacian = matches!(card, Card::DerivativeCard(DerivativeCard::Laplacian));
    let is_integral = matches!(card, Card::DerivativeCard(DerivativeCard::Integral));
    let is_derivative = matches!(
        card,
        Card::DerivativeCard(DerivativeCard::Derivative | DerivativeCard::Nabla)
    );

    let selected_field_basis = &field[i];
    if selected_field_basis.basis.is_none() {
        return;
    }

    // shortcut if already in history
    if selected_field_basis.has_value(&card) {
        if is_derivative || is_laplacian {
            field.derivative(i, None);
        } else if is_integral {
            field.integral(i, None);
        }
        if is_laplacian {
            field.derivative(i, None);
        }
    } else {
        // calculate derivative/integral
        let result_basis = apply_card(&card)(field[i].basis.as_ref().unwrap());
        if result_basis.is_num(0) {
            field[i] = FieldBasis::none();
            return;
        } else {
            if is_derivative || is_laplacian {
                field.derivative(i, Some(result_basis.clone()));
            } else if is_integral {
                field.integral(i, Some(result_basis.clone()));
            }
        }
        // calculate second derivative if laplacian
        if is_laplacian {
            let second_derivative = apply_card(&card)(&result_basis);
            if second_derivative.is_num(0) {
                field[i] = FieldBasis::none();
                return;
            }
            field.derivative(i, Some(second_derivative));
        }
    }
}

/// handles multiselect turn phase, player can choose multiple targets of selected operator (Mult/Div)
/// Only allows field+hand combinations (field×field is deprecated)
fn multi_select_phase(multi_operator: Card, id: RenderId, player_num: u32) {
    let game = unsafe { GAME.as_mut().unwrap() };
    let player = if &game.turn.number % 2 == 0 {
        &mut game.player_1
    } else {
        &mut game.player_2
    };
    let field = &game.field;
    let selected = &mut game.active.selected;
    let (id_key, id_val) = id.key_val();

    // Only allow field+hand combinations, not field×field
    // Count how many field slots are already selected
    let field_count = selected.iter().filter(|sel_id| sel_id.is_field()).count();
    
    // add selected field Basis to list of active selections (only if no field selected yet)
    if id_key == "f" && field_count == 0 {
        selected.push(id);
        render::draw();
        // console::log_1(&JsValue::from(format!("added to multiselect: {}", id)));
    } else if id_key == format!("p{}", player_num) && matches!(player[id_val], Card::BasisCard(_)) {
        // Allow hand cards
        selected.push(id);
        render::draw();
    }

    let has_at_least_1_field_basis = selected.iter().find(|sel_id| sel_id.is_field()).is_some();
    let has_at_least_2_basis = selected.len() >= 3; // add one for operator
    let has_zero_with_many = selected.len() != 3
        && selected
            .iter()
            .find(|sel_id| {
                sel_id.is_player() // must be a player card
                    && matches!( // corresponding player card is zero
                        player[sel_id.key_val().1],
                        Card::BasisCard(BasisCard::Zero)
                    )
            })
            .is_some();

    if (id_key == "x" && id_val == 1)
        && has_at_least_1_field_basis
        && !has_zero_with_many
        && has_at_least_2_basis
    {
        let result_basis = apply_multi_card(
            &multi_operator,
            selected
                .iter()
                .filter_map(|sel_id| {
                    let (sel_key, sel_val) = sel_id.key_val();

                    if sel_key == "f" {
                        return Some(field[sel_val].basis.as_ref().unwrap().clone());
                    } else if sel_key == format!("p{}", player_num) {
                        if let Card::AlgebraicCard(_operator) = player[sel_val] {
                            // skip the mult_operator
                            return None;
                        } else if let Card::BasisCard(basis_card) = player[sel_val] {
                            return Some(Basis::from(basis_card));
                        }
                    }
                    panic!("invalid card selected! {}", sel_id);
                })
                .collect::<Vec<Basis>>(),
        );
        // get references to all selected cards
        let used_field_bases = selected
            .iter()
            .filter_map(|sel_id| {
                let (sel_key, sel_val) = sel_id.key_val();
                if sel_key == "f" {
                    return Some(sel_val);
                }
                None
            })
            .collect::<Vec<usize>>();
        let mut new_field = field.clone();
        used_field_bases // clear used field bases
            .iter()
            .for_each(|field_index| new_field[*field_index] = FieldBasis::none());
        if result_basis.is_num(0) {
            new_field[used_field_bases[0]] = FieldBasis::none();
        } else {
            new_field[used_field_bases[0]] = FieldBasis::new(&result_basis); // assign result basis to any newly empty field
        }
        commit_or_confirm(new_field, used_field_bases);
    }
}

/// performs cleanup tasks after turn is over
fn end_turn() {
    let game = unsafe { GAME.as_mut().unwrap() };

    if matches!(game.state, GameState::PLAYAI) {
        crate::game::match_log::flush_turn(game.get_current_player_num());
    }

    // get vector indices of cards used by player this turn
    let mut selected_indices = game
        .active
        .selected
        .iter()
        .filter(|card| card.is_player())
        .map(|card| card.key_val().1)
        .collect::<Vec<usize>>();
    selected_indices.sort();
    selected_indices.reverse();

    let player_num = game.get_current_player_num();
    let player = if player_num == 1 {
        &mut game.player_1
    } else {
        &mut game.player_2
    };
    // remove used cards
    for i in selected_indices.iter() {
        let used_card = player.remove(*i);
        game.graveyard.push(used_card);
    }

    let deck = &mut game.deck;
    // replenish from deck if possible -- saturating_sub guards against a hand
    // somehow already at or past 7 (shouldn't happen, but a plain `7 -
    // player.len()` would panic on underflow in that case instead of just
    // correctly dealing zero cards)
    let cards_to_deal = 7usize.saturating_sub(player.len()).min(deck.len());
    anim::animate_deal_cards(
        (player.len()..player.len() + cards_to_deal)
            .map(|i| {
                RenderId::from(format!(
                    "p{player_num}={val}",
                    player_num = player_num,
                    val = i
                ))
            })
            .collect::<Vec<RenderId>>(),
    );

    let flag = unsafe { ALLOW_LINEAR_DEPENDENCE };
    if !flag {
        // TODO: animate ?
        game.field.clear_linearly_dependent_pairs();
    }

    next_turn();
}

/// shifts to next turn phase with given selected card
fn next_phase(phase: TurnPhase) {
    let game = unsafe { GAME.as_mut().unwrap() };
    // console::log_1(&JsValue::from(format!("entering phase: {:?}", phase)));
    game.turn = Turn {
        number: game.turn.number,
        phase: phase,
    };
    render::draw();
}

/// finalises turn and increments turn, checking if game is in terminal state
pub fn next_turn() {
    let game = unsafe { GAME.as_mut().unwrap() };

    // console::log_1(&JsValue::from(format!("entering turn: {}", game.turn.number + 1)));
    game.turn = Turn {
        number: game.turn.number + 1,
        phase: TurnPhase::IDLE,
    };
    game.active.clear();
    render::draw();

    if matches!(game.state, GameState::PLAYONLINE) {
        crate::game::online::flush_outgoing_action();
    }

    let field = game.field.basis.iter();
    if field.clone().take(3).all(|f| f.basis.is_none()) {
        // player 1 wins
        game.game_over(1);
    } else if field.clone().skip(3).all(|f| f.basis.is_none()) {
        // player 2 wins
        game.game_over(2);
    } else {
        crate::game::ai::maybe_take_ai_turn();
    }
}
