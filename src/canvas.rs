// std imports
use std::collections::{HashMap, HashSet};
// wasm-bindgen imports
use gloo::events::EventListener;
use gloo::render::request_animation_frame;
use wasm_bindgen::JsCast;
use web_sys::*;
// outer crate imports
use crate::render::anim::{on_animation_frame, AnimController};
use crate::render::pos::*;
use crate::render::sprites::SpriteLookup;
use crate::render::util::*;
// util imports
use crate::util::{min, Vector2};

/// Controller for canvas elements, related contexts, and event listeners
pub struct Canvas {
    pub canvas_element: HtmlCanvasElement,
    pub context: CanvasRenderingContext2d,
    pub canvas_bounds: Vector2,
    pub canvas_center: Vector2,

    pub hit_canvas_element: HtmlCanvasElement,
    pub hit_context: CanvasRenderingContext2d,
    pub hit_region_map: HashMap<String, String>,

    pub mousedown_listener: Option<EventListener>,
    pub mousemove_listener: Option<EventListener>,

    pub render_constants: RenderConstants,
    pub render_items: RenderHash,
    pub sprite_element: HtmlImageElement,
    pub sprite_lookup: SpriteLookup,

    pub anim_controller: AnimController,
    // pub render_animation_frame_handle: AnimationFrame,
    // pub anim_items: HashMap<RenderId, AnimItem>,
    /// field cards tapped open to reveal their full (otherwise clipped) expression
    pub expanded_cards: HashSet<RenderId>,
}

impl Canvas {
    /// get canvases from DOM and extract client bounds and center
    pub fn new(document: &Document) -> Canvas {
        let canvas_element: HtmlCanvasElement = document
            .get_element_by_id("canvas")
            .unwrap()
            .dyn_into()
            .unwrap();
        let hit_canvas_element: HtmlCanvasElement = document
            .get_element_by_id("hitCanvas")
            .unwrap()
            .dyn_into()
            .unwrap();
        let sprite_element: HtmlImageElement = document
            .get_element_by_id("spritesheet")
            .unwrap()
            .dyn_into()
            .unwrap();

        let context = canvas_element
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();
        let hit_context = hit_canvas_element
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();

        let hit_region_map = HashMap::new();

        let canvas_bounds = Vector2 {
            x: f64::from(canvas_element.width()),
            y: f64::from(canvas_element.height()),
        };

        let canvas_center = Vector2 {
            x: canvas_bounds.x / 2.0,
            y: canvas_bounds.y / 2.0,
        };

        Canvas {
            canvas_element,
            context,
            canvas_bounds,
            canvas_center,
            hit_canvas_element,
            hit_context,
            hit_region_map,
            mousedown_listener: None,
            mousemove_listener: None,
            render_constants: RenderConstants {
                field_sizes: Sizes::default(),
                player_sizes: Sizes::default(),
                button_sizes: Sizes::default(),
                sprite_scale: 1.0,
            },
            render_items: HashMap::default(),
            sprite_element,
            sprite_lookup: SpriteLookup::new(),
            anim_controller: AnimController {
                anim_items: HashMap::default(),
                anim_chain: HashMap::default(),
                render_animation_frame_handle: request_animation_frame(on_animation_frame),
            },
            expanded_cards: HashSet::default(),
        }
    }

    /// recalculate canvas element sizes on resize
    pub fn resize(&mut self) {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        let inner_width = window.inner_width().unwrap().as_f64().unwrap() as u32;
        let inner_height = window.inner_height().unwrap().as_f64().unwrap() as u32;

        self.canvas_element.set_width(inner_width);
        self.canvas_element.set_height(inner_height);
        self.hit_canvas_element.set_width(inner_width);
        self.hit_canvas_element.set_height(inner_height);

        self.rebounds();
        self.update_render_constants();

        // if the current layout needs more vertical room than the viewport actually
        // has (eg. a short window, or mobile with its taller two-row hand layout),
        // grow the canvas to fit everything at a usable size instead of squeezing
        // cards down indefinitely -- the page then scrolls to reach the rest, rather
        // than clipping or overlapping content
        let required_height = self.required_canvas_height();
        if required_height > self.canvas_bounds.y {
            let grown_height = required_height.ceil() as u32;
            self.canvas_element.set_height(grown_height);
            self.hit_canvas_element.set_height(grown_height);
            self.rebounds();
        }

        // CSS alone can't know the canvas's own computed pixel height, so the
        // scrollable body's min-height is driven from here directly
        if let Some(body) = document.body() {
            body.style()
                .set_property("min-height", &format!("{}px", self.canvas_bounds.y))
                .ok();
        }

        self.calculate_render_positions();
    }

    /// the total vertical space the current layout needs to render every element
    /// without overlap, at the sizes update_render_constants just computed -- see
    /// resize's scroll-fallback comment for why this matters
    fn required_canvas_height(&self) -> f64 {
        let Sizes {
            height: player_card_height,
            gutter: player_card_gutter,
            ..
        } = self.render_constants.player_sizes;
        let Sizes {
            height: field_basis_height,
            gutter: field_basis_gutter,
            ..
        } = self.render_constants.field_sizes;

        let is_mobile = is_mobile_layout(self.canvas_bounds.x, self.canvas_bounds.y);
        let hand_zone = hand_zone_height(is_mobile, player_card_height, player_card_gutter);

        hand_zone * 2.0 // both players' hand zones
            + player_card_gutter * 2.0 // edge margins around each hand zone
            + field_basis_height * 2.0 // the field's own two rows
            + field_basis_gutter * 3.0 // gutters around/between the field rows
    }

    /// recalculate canvas bounds and center on resize
    fn rebounds(&mut self) {
        let canvas_bounds = Vector2 {
            x: f64::from(self.canvas_element.width()),
            y: f64::from(self.canvas_element.height()),
        };

        let canvas_center = Vector2 {
            x: canvas_bounds.x / 2.0,
            y: canvas_bounds.y / 2.0,
        };

        self.canvas_bounds = canvas_bounds;
        self.canvas_center = canvas_center;
    }

    /// update sizes for player cards and field bases
    fn update_render_constants(&mut self) {
        let is_mobile = is_mobile_layout(self.canvas_bounds.x, self.canvas_bounds.y);

        // mobile splits the 7-card hand into two rows (3 inner + 4 outer) instead of
        // one row of 7 (see pos::get_base_player_pos), so cards are sized to fit the
        // wider of the two rows -- 4 across -- rather than 7 across; desktop is
        // completely unchanged (still a fixed 9rem card height)
        let (player_card_width, player_card_height) = if is_mobile {
            let available_width = self.canvas_bounds.x * 0.94;
            // 4 cards + 3 gutters, gutter = width/4 => 4w + 3(w/4) = 4.75w
            let width = (available_width / 4.75).clamp(48.0, 90.0);
            (width, width / 0.75)
        } else {
            let height = rem_to_px(String::from("9rem"));
            (height * 0.75, height)
        };
        let gutter = player_card_width / 4.0;
        let radius = gutter / 4.0;

        let hand_zone = hand_zone_height(is_mobile, player_card_height, gutter);

        let field_gutter = gutter * 2.0;
        let field_basis_height = min(
            (self.canvas_bounds.y - hand_zone * 2.0 - gutter * 2.0 - field_gutter * 3.0) / 2.0, // distance from edge of player to center
            player_card_height * 2.0,
        )
        .max(if is_mobile { 40.0 } else { 0.0 }); // never collapse to nothing on mobile; resize's scroll-fallback grows the canvas instead
        let field_basis_width = field_basis_height * 0.75;
        let field_radius = field_gutter / 4.0;

        self.render_constants = RenderConstants {
            field_sizes: Sizes {
                width: field_basis_width,
                height: field_basis_height,
                gutter: field_gutter,
                radius: field_radius,
            },
            player_sizes: Sizes {
                width: player_card_width,
                height: player_card_height,
                gutter,
                radius,
            },
            button_sizes: Sizes {
                width: if is_mobile {
                    player_card_width * 0.6
                } else {
                    player_card_width
                },
                height: if is_mobile {
                    // fit comfortably within the strip reserved for it (see
                    // mobile_button_strip_height, used both here and by
                    // hand_zone_height above) rather than an independently-tuned
                    // number that could silently drift out of sync with it
                    mobile_button_strip_height(player_card_height, gutter) * 0.75
                } else {
                    (player_card_height - gutter) / 2.0
                },
                gutter,
                radius: radius / 2.0,
            },
            sprite_scale: self.sprite_lookup.card_height / player_card_height,
        };
    }

    /// calculate default render positions for all render items
    fn calculate_render_positions(&mut self) {
        self.render_items.clear();
        let field_pos = get_base_field_pos();
        let player_pos = get_base_player_pos();
        let button_pos = get_base_button_pos(&field_pos, &player_pos);
        self.render_items.extend(field_pos);
        self.render_items.extend(player_pos);
        self.render_items.extend(button_pos);
    }
}
