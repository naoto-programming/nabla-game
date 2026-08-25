// std imports
use std::collections::HashMap;
// external crate imports
use crate::util::*;
use crate::{CANVAS, GAME};
// internal crate imports
use super::util::*;

pub fn get_base_player_pos() -> RenderHash {
    let canvas = unsafe { CANVAS.as_mut().unwrap() };
    let center = &canvas.canvas_center;
    let bounds = &canvas.canvas_bounds;
    let is_mobile = is_mobile_layout(bounds.x, bounds.y);

    let Sizes {
        width: player_card_width,
        height: player_card_height,
        gutter: player_card_gutter,
        radius: player_card_radius,
    } = canvas.render_constants.player_sizes;

    let mut player_pos: RenderHash = HashMap::new();
    for player_num in 1..=2 {
        let is_bottom = player_renders_at_bottom(player_num);

        if is_mobile {
            // hand split into an inner row (3 cards, nearer the field) and an outer
            // row (4 cards, nearer the screen edge) instead of one row of 7 -- there
            // isn't enough width on a phone in portrait for 7 cards across
            let outer_y = if is_bottom {
                bounds.y - player_card_height - player_card_gutter
            } else {
                // extra clearance so this row doesn't sit under the fixed Main
                // Menu / language-toggle buttons (see MOBILE_TOP_SAFE_AREA_PX)
                MOBILE_TOP_SAFE_AREA_PX
            };
            let inner_y = if is_bottom {
                outer_y - player_card_gutter - player_card_height
            } else {
                outer_y + player_card_gutter + player_card_height
            };

            let inner_start_x = center.x - 1.5 * player_card_width - player_card_gutter;
            let outer_start_x = center.x - 2.0 * player_card_width - 1.5 * player_card_gutter;

            for i in 0..3 {
                player_pos.insert(
                    RenderId::from(format!("p{player_num}={i}")),
                    RenderItem {
                        x: inner_start_x + (i as f64) * (player_card_width + player_card_gutter),
                        y: inner_y,
                        w: player_card_width,
                        h: player_card_height,
                        r: player_card_radius,
                    },
                );
            }
            for i in 3..7 {
                let j = (i - 3) as f64;
                player_pos.insert(
                    RenderId::from(format!("p{player_num}={i}")),
                    RenderItem {
                        x: outer_start_x + j * (player_card_width + player_card_gutter),
                        y: outer_y,
                        w: player_card_width,
                        h: player_card_height,
                        r: player_card_radius,
                    },
                );
            }
            continue;
        }

        let start_pos = Vector2 {
            x: center.x - 3.5 * player_card_width - 3.0 * player_card_gutter,
            y: if is_bottom {
                bounds.y - player_card_height - player_card_gutter // bottom of canvas
            } else {
                player_card_gutter // top of canvas
            },
        };

        for i in 0..7 {
            player_pos.insert(
                RenderId::from(format!("p{player_num}={i}")),
                RenderItem {
                    x: start_pos.x + (i as f64) * (player_card_width + player_card_gutter),
                    y: start_pos.y,
                    w: player_card_width,
                    h: player_card_height,
                    r: player_card_radius,
                },
            );
        }
    }

    player_pos
}

pub fn get_hover_player_pos(player_num: u32, hover_val: usize) -> RenderHash {
    let canvas = unsafe { CANVAS.as_ref().unwrap() };

    // touch devices don't have a meaningful hover state, and there's no spare room
    // in the mobile two-row hand layout for a card to grow into anyway -- skip the
    // hover-grow effect there and just keep every card at its base position
    if is_mobile_layout(canvas.canvas_bounds.x, canvas.canvas_bounds.y) {
        return get_base_player_pos();
    }

    let Sizes {
        width: player_card_width,
        height: player_card_height,
        gutter: player_card_gutter,
        radius: player_card_radius,
    } = canvas.render_constants.player_sizes;

    let start_pos = Vector2 {
        x: canvas.canvas_center.x
            - (
                (player_card_gutter * 7.0 + player_card_width * 7.0)
                // width of 6 cards
            ) / 2.0, // divide by 2 for distance from center
        y: if player_renders_at_bottom(player_num) {
            canvas.canvas_bounds.y - player_card_gutter - player_card_height // bottom of canvas
        } else {
            player_card_gutter // top of canvas
        },
    };

    let mut player_pos: RenderHash = HashMap::new();
    for i in 0..7 {
        let extra_size = if i == hover_val {
            player_card_gutter
        } else {
            0.0
        };
        player_pos.insert(
            RenderId::from(format!("p{player_num}={i}")),
            RenderItem {
                x: start_pos.x
                + (i as f64) * (player_card_width + player_card_gutter)
                // add extra space for cards after hover
                + if i > hover_val {
                    player_card_gutter
                } else {
                    0.0
                },
                y: start_pos.y - if player_renders_at_bottom(player_num) { extra_size } else { 0.0 },
                w: player_card_width + extra_size,
                h: player_card_height + extra_size,
                r: player_card_radius,
            },
        );
    }

    player_pos
}

pub fn get_base_field_pos() -> RenderHash {
    let canvas = unsafe { CANVAS.as_mut().unwrap() };
    let center = &canvas.canvas_center;

    let Sizes {
        width: field_basis_width,
        height: field_basis_height,
        gutter: field_basis_gutter,
        radius: field_basis_radius,
    } = canvas.render_constants.field_sizes;

    let mut field_pos: RenderHash = HashMap::new();
    for i in 0..6 {
        // slots 0-2 render in player 2's colour, 3-5 in player 1's (see draw()) --
        // whichever of those two players is rendering at the bottom this turn should
        // have their field half in the bottom row too, so the board mirrors the hand
        // that's near it, instead of always fixing 0-2 to the top row regardless of
        // perspective
        let owning_player = if i < 3 { 2 } else { 1 };
        let row = if player_renders_at_bottom(owning_player) {
            1
        } else {
            0
        };
        field_pos.insert(
            RenderId::from(format!("f={i}")),
            RenderItem {
                x: center.x + ((i % 3) as f64) * (field_basis_width + field_basis_gutter)
                    - field_basis_width * 1.5
                    - field_basis_gutter,
                y: center.y + (row as f64) * (field_basis_height + field_basis_gutter)
                    - field_basis_height
                    - field_basis_gutter / 2.0,
                w: field_basis_width,
                h: field_basis_height,
                r: field_basis_radius,
            },
        );
    }

    field_pos
}

pub fn get_base_button_pos(field_pos: &RenderHash, player_pos: &RenderHash) -> RenderHash {
    let (canvas, game) = unsafe { (CANVAS.as_ref().unwrap(), GAME.as_ref().unwrap()) };
    let center = &canvas.canvas_center;
    let player_num = game.get_current_player_num();

    let Sizes {
        width: field_basis_width,
        height: field_basis_height,
        gutter: field_basis_gutter,
        radius: field_basis_radius,
    } = canvas.render_constants.field_sizes;
    let Sizes {
        height: button_height,
        gutter: button_gutter,
        radius: button_radius,
        width: button_width,
    } = canvas.render_constants.button_sizes;

    let deck_pos = RenderItem {
        x: field_pos[&RenderId::Field0].x - field_basis_width - field_basis_gutter,
        y: center.y - field_basis_height / 2.0,
        w: field_basis_width,
        h: field_basis_height,
        r: field_basis_radius,
    };

    if is_mobile_layout(canvas.canvas_bounds.x, canvas.canvas_bounds.y) {
        // no horizontal room beside the (narrower) hand rows on mobile -- instead,
        // Cancel/TurnIndicator/Multidone-or-Confirm sit in a row in the button strip
        // reserved between the current player's inner hand row and the field (see
        // hand_zone_height / mobile_button_strip_height, which size that gap)
        let is_bottom = player_renders_at_bottom(player_num);
        let inner_row = &player_pos[&RenderId::from(format!("p{player_num}=0"))];
        let y = if is_bottom {
            inner_row.y - button_gutter - button_height
        } else {
            inner_row.y + inner_row.h + button_gutter
        };

        // deck, turn indicator, cancel, and multidone/confirm all share this one
        // strip on mobile (there's no room beside the field for the deck either, the
        // way desktop places it) -- 4 button-sized items centered as a group
        let strip_start_x = center.x - 2.0 * button_width - 1.5 * button_gutter;
        let mobile_deck_pos = RenderItem {
            x: strip_start_x,
            y,
            w: button_width,
            h: button_height,
            r: button_radius,
        };
        let turn_indicator_pos = RenderItem {
            x: strip_start_x + (button_width + button_gutter),
            y,
            w: button_width,
            h: button_height,
            r: button_radius,
        };
        let cancel_pos = RenderItem {
            x: strip_start_x + 2.0 * (button_width + button_gutter),
            y,
            w: button_width,
            h: button_height,
            r: button_radius,
        };
        let multidone_pos = RenderItem {
            x: strip_start_x + 3.0 * (button_width + button_gutter),
            y,
            w: button_width,
            h: button_height,
            r: button_radius,
        };
        // Confirm reuses Multidone's slot: the two are never shown on the same turn phase
        let confirm_pos = multidone_pos;

        return HashMap::from([
            (RenderId::Deck, mobile_deck_pos),
            (RenderId::Cancel, cancel_pos),
            (RenderId::Multidone, multidone_pos),
            (RenderId::Confirm, confirm_pos),
            (RenderId::TurnIndicator, turn_indicator_pos),
        ]);
    }

    let cancel_pos = RenderItem {
        x: player_pos[&RenderId::PlayerOne6].x + button_width + button_gutter,
        y: player_pos[&RenderId::from(format!("p{player_num}=0"))].y,
        w: button_width,
        h: button_height,
        r: button_radius,
    };

    let multidone_pos = RenderItem {
        x: player_pos[&RenderId::PlayerOne6].x + button_width + button_gutter,
        y: player_pos[&RenderId::from(format!("p{player_num}=0"))].y
            + button_height
            + button_gutter,
        w: button_width,
        h: button_height,
        r: button_radius,
    };

    let turn_indicator_pos = RenderItem {
        x: player_pos[&RenderId::PlayerOne0].x - button_width - button_gutter,
        y: player_pos[&RenderId::from(format!("p{player_num}=0"))].y,
        w: button_width,
        h: button_height,
        r: button_radius,
    };

    // Confirm reuses Multidone's slot: the two are never shown on the same turn phase
    let confirm_pos = multidone_pos;

    HashMap::from([
        (RenderId::Deck, deck_pos),
        (RenderId::Cancel, cancel_pos),
        (RenderId::Multidone, multidone_pos),
        (RenderId::Confirm, confirm_pos),
        (RenderId::TurnIndicator, turn_indicator_pos),
    ])
}
