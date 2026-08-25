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

/// the field_basis_height that a given player_card_width actually renders at (card
/// height = width / 0.75, the fixed aspect ratio) -- the one authoritative
/// implementation of this calculation, used both to pick a card width (see
/// find_max_mobile_card_width) and to build the real RenderConstants
/// (update_render_constants). Earlier this was duplicated (once inline in
/// update_render_constants, once approximated in a separate linear formula this
/// function replaced) and every new constraint added to one needed a matching
/// update to the other -- each miss produced a new overlap edge case on some
/// specific phone aspect ratio. A single implementation can't drift from itself.
fn field_basis_height_for(card_width: f64, is_mobile: bool, bounds: &Vector2) -> f64 {
    let card_height = card_width / 0.75;
    let gutter = card_width / 4.0;
    let hand_zone = hand_zone_height(is_mobile, card_height, gutter);
    let field_gutter = gutter * 2.0;
    // on a phone with generous height relative to its width (eg. tall and narrow),
    // the vertical cap alone (card_height * 2.0) could still produce a field wide
    // enough (3 columns, field_basis_width = height * 0.75) to overflow the
    // viewport horizontally -- desktop never hits this (its fixed 9rem card height
    // was already tuned to fit typical desktop widths), so this only applies on
    // mobile
    let field_width_cap = if is_mobile {
        // 3 columns + 2 gutters between them must fit: 3w + 2*field_gutter <=
        // available width, with w = height * 0.75
        ((bounds.x * 0.94 - field_gutter * 2.0) / 3.0) / 0.75
    } else {
        f64::MAX
    };
    // exactly one of the two hand zones ends up on top regardless of which player
    // that turns out to be (see player_renders_at_bottom) -- on mobile that top
    // margin is the larger safe area (MOBILE_TOP_SAFE_AREA_PX), not the plain
    // gutter (this must match pos.rs's actual placement, and total_required_height_for
    // below). Using a plain gutter * 2.0 here (as an earlier version did) reserves
    // ~100px too little space above the top hand's inner row -- the field then
    // renders that much taller than the room actually left for it, overlapping the
    // opponent's inner row from below even though the *total* page height still
    // fit (find_max_mobile_card_width's check uses this same top_margin, and
    // shrinking the whole layout to compensate doesn't fix a fixed-pixel deficit
    // localized to one specific gap)
    let top_margin = if is_mobile { MOBILE_TOP_SAFE_AREA_PX } else { gutter };

    min(
        min(
            (bounds.y - hand_zone * 2.0 - gutter - top_margin - field_gutter * 3.0) / 2.0, // distance from edge of player to center
            card_height * 2.0,
        ),
        field_width_cap,
    )
    .max(if is_mobile { 40.0 } else { 0.0 }) // never collapse to nothing on mobile; resize's scroll-fallback grows the canvas instead
}

/// total vertical space needed to give a player_card_width a *comfortable* layout
/// (eg. not the current, possibly floor-squeezed field_basis_height_for result, but
/// what it would take for the field to reach a reasonable size) -- used both to
/// find the largest card width that needs no scrolling at all (see
/// find_max_mobile_card_width) and to size the scroll-fallback growth in resize()
/// when even the smallest allowed card width still doesn't fit comfortably
fn total_required_height_for(card_width: f64, is_mobile: bool, bounds: &Vector2) -> f64 {
    let card_height = card_width / 0.75;
    let gutter = card_width / 4.0;
    let hand_zone = hand_zone_height(is_mobile, card_height, gutter);
    let field_gutter = gutter * 2.0;

    let current_field_height = field_basis_height_for(card_width, is_mobile, bounds);
    let field_cap = card_height * 2.0;
    // below ~90% of a hand card's own height reads as uncomfortably small; a normal
    // desktop window's naturally-computed field height sits well above this, so it
    // never escalates on desktop
    let squeeze_threshold = card_height * 0.9;
    let field_target = if current_field_height < squeeze_threshold {
        field_cap
    } else {
        current_field_height
    };
    // exactly one of the two hand zones ends up on top regardless of which player
    // that turns out to be (see player_renders_at_bottom) -- on mobile that top
    // margin is the larger safe area (MOBILE_TOP_SAFE_AREA_PX), not the plain
    // gutter, so this must match pos.rs's actual placement
    let top_margin = if is_mobile { MOBILE_TOP_SAFE_AREA_PX } else { gutter };

    hand_zone * 2.0 // both players' hand zones
        + gutter // bottom edge margin
        + top_margin // top edge margin
        + field_target * 2.0 // the field's own two rows
        + field_gutter * 3.0 // gutters around/between the field rows
}

/// binary-searches for the largest mobile card width (within [28, 90]) whose
/// *comfortable* layout (see total_required_height_for) fits within bounds.y
/// without needing to scroll at all -- replaces solving an approximate linear
/// formula for this, which needed to be kept in perfect sync with every constraint
/// in the real sizing logic and silently drifted whenever one changed
fn find_max_mobile_card_width(bounds: &Vector2) -> f64 {
    let (mut lo, mut hi) = (28.0_f64, 90.0_f64);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if total_required_height_for(mid, true, bounds) <= bounds.y {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
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
            // field_basis_height (and anything else derived from canvas_bounds.y) was
            // computed against the old, too-short height above -- without this, the
            // field would stay squeezed down to its floor size even though the canvas
            // was just grown specifically to give it room, and KaTeX's fixed-size text
            // would overflow/overlap those undersized cards
            self.update_render_constants();
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

    /// the total vertical space the current layout needs -- see resize's
    /// scroll-fallback comment for why this matters. Delegates entirely to
    /// total_required_height_for using the card width already committed to
    /// render_constants, so this can never drift from the sizing logic that
    /// picked that width in the first place (see that function's doc for why that
    /// matters: on a normal desktop window the field naturally computes to
    /// something comfortably large but well under its cap, so this does NOT grow
    /// the canvas to force it to that cap -- doing so previously grew the canvas
    /// ~80px taller than the viewport on every ordinary desktop window, silently
    /// shifting the whole bottom-anchored hand down)
    fn required_canvas_height(&self) -> f64 {
        let is_mobile = is_mobile_layout(self.canvas_bounds.x, self.canvas_bounds.y);
        total_required_height_for(
            self.render_constants.player_sizes.width,
            is_mobile,
            &self.canvas_bounds,
        )
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
            let width_from_width = (available_width / 4.75).clamp(48.0, 90.0);

            // also cap by height: sizing purely from width ignores the device's
            // actual aspect ratio -- a phone with less generous height relative to
            // its width would size cards as if it had full headroom and then lean
            // entirely on resize()'s scroll-fallback to fit everything, which reads
            // as mismatched rather than fitted to the device. find_max_mobile_card_width
            // binary-searches total_required_height_for -- the same authoritative
            // sizing calculation this function builds RenderConstants from below -- so
            // this can't silently drift out of sync with it the way a hand-derived
            // approximation could
            let width_from_height = find_max_mobile_card_width(&self.canvas_bounds);

            // the lower bound here must never override width_from_height upward: doing
            // so would size cards larger than the height budget actually allows, and
            // since resize()'s growth target is computed FROM this same (now
            // artificially inflated) card size, growth would follow the inflation
            // rather than correct it, leaving the layout genuinely too tall for the
            // grown canvas -- this is exactly what produced real overlap on short
            // viewports before. 28px is a last-resort-only floor for truly degenerate
            // sizes; width_from_height staying below it is not expected in practice
            let width = width_from_width.min(width_from_height).clamp(28.0, 90.0);
            (width, width / 0.75)
        } else {
            let height = rem_to_px(String::from("9rem"));
            (height * 0.75, height)
        };
        let gutter = player_card_width / 4.0;
        let radius = gutter / 4.0;

        let field_gutter = gutter * 2.0;
        let field_basis_height =
            field_basis_height_for(player_card_width, is_mobile, &self.canvas_bounds);
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

        // the field is drawn straddling canvas_center.y (see get_base_field_pos), so
        // that point must sit at the true midpoint between the two hand zones -- not
        // at the canvas's own geometric midpoint. On mobile those two zones are NOT
        // mirror images of each other: the top (opponent) zone's margin is the large
        // MOBILE_TOP_SAFE_AREA_PX clearance under the fixed menu buttons, while the
        // bottom (player) zone's margin is a plain gutter (its button strip lives
        // below the field, inside the field/hand gap, not above the hand). Leaving
        // canvas_center.y at a plain bounds.y/2.0 split the leftover space evenly
        // regardless of this asymmetry, which pushed the field upward into the
        // opponent's hand by roughly half that safe-area difference. On desktop both
        // zones use the same plain-gutter margin, so this correction is exactly zero
        // and canvas_center.y reduces to the plain midpoint as before.
        let top_zone_height = if is_mobile {
            MOBILE_TOP_SAFE_AREA_PX + player_card_height * 2.0 + gutter
        } else {
            gutter + player_card_height
        };
        let bottom_zone_height = if is_mobile {
            player_card_height * 2.0 + gutter * 2.0
        } else {
            player_card_height + gutter
        };
        self.canvas_center.y =
            self.canvas_bounds.y / 2.0 + (top_zone_height - bottom_zone_height) / 2.0;
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
