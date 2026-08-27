// std imports
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
// wasm-bindgen imports
use gloo::render::{request_animation_frame, AnimationFrame};
// local imports
use super::pos;
use super::render;
use super::util::{RenderId, RenderItem};
// root imports
use crate::{CANVAS, GAME};
// util imports
use crate::util::min;

/// requestAnimationFrame callback
pub fn on_animation_frame(time: f64) {
    let canvas = unsafe { CANVAS.as_mut().unwrap() };
    let anim_controller = &mut canvas.anim_controller;
    let anim_items = &mut anim_controller.anim_items;
    let mut finished: Vec<RenderId> = Vec::new();

    for (id, anim_item) in anim_items {
        if anim_item.start.is_none() {
            anim_item.start = Some(time);
        }

        let mut current = RenderItem::default();
        // simple lerp
        let delta = min::<f64>(
            (time - anim_item.start.unwrap()) / anim_item.duration / 1000.0,
            1.0,
        );
        for (attr, val) in anim_item.attributes.iter() {
            let (start, end) = val;
            current[*attr] = start + delta * (end - start);
        }
        if delta >= 1.0 {
            finished.push(*id);
        }

        canvas.render_items.insert(*id, current);
        render::draw();
    }

    for id in finished {
        let removed = anim_controller.anim_items.remove(&id).unwrap();
        removed.callback.iter().for_each(|f| f());

        // pops the next queued animation for this id, if any -- and removes the
        // map entry entirely once its queue is drained (rather than leaving an
        // empty Vec behind). RenderIds get reused constantly (only 7 hand slots
        // per player, recycled every time a card is dealt into one), so without
        // this cleanup, a later unrelated animation on that same id would find
        // contains_key() still true from the stale, already-empty entry and
        // panic on remove(0) -- "removal index (is 0) should be < len (is 0)"
        let mut queue_now_empty = false;
        if let Some(anim_queue) = anim_controller.anim_chain.get_mut(&id) {
            if !anim_queue.is_empty() {
                let next_anim = anim_queue.remove(0);
                anim_controller.anim_items.insert(id, next_anim);
            }
            queue_now_empty = anim_queue.is_empty();
        }
        if queue_now_empty {
            anim_controller.anim_chain.remove(&id);
        }
    }

    if canvas.anim_controller.anim_items.len() > 0 {
        canvas.anim_controller.render_animation_frame_handle =
            request_animation_frame(on_animation_frame);
    }
}

/// starts hover animation on player cards
pub fn animate_hover(id: Option<RenderId>) {
    let canvas = unsafe { CANVAS.as_mut().unwrap() };
    let render_items = &canvas.render_items;

    let target_pos = if id.is_some() {
        let (key, val) = id.unwrap().key_val();
        let player_num = key.chars().nth(1).unwrap().to_digit(10).unwrap();
        pos::get_hover_player_pos(player_num, val)
    } else {
        pos::get_base_player_pos()
    };

    canvas
        .anim_controller
        .anim_items
        .extend(target_pos.iter().map(|(id, item)| {
            (
                *id,
                AnimItem {
                    start: None,
                    duration: 0.1,
                    attributes: HashMap::from([
                        (AnimAttribute::X, (render_items[id].x, item.x)),
                        (AnimAttribute::Y, (render_items[id].y, item.y)),
                        (AnimAttribute::W, (render_items[id].w, item.w)),
                        (AnimAttribute::H, (render_items[id].h, item.h)),
                        (AnimAttribute::R, (render_items[id].r, item.r)),
                    ]),
                    callback: vec![],
                },
            )
        }));

    canvas.anim_controller.start_anim();
}

pub fn animate_deal_cards(ids: Vec<RenderId>) {
    let canvas = unsafe { CANVAS.as_mut().unwrap() };
    let anim_controller = &mut canvas.anim_controller;

    for (i, id) in ids.iter().enumerate() {
        let (deal_id, anim_item) = animate_deal(*id);
        
        // Chain animations: each deal triggers the next one
        if i < ids.len() - 1 {
            let next_anim = animate_deal(ids[i + 1]).1;
            if anim_controller.anim_chain.contains_key(&deal_id) {
                anim_controller
                    .anim_chain
                    .entry(deal_id)
                    .or_default()
                    .push(next_anim);
            } else {
                anim_controller.anim_chain.insert(deal_id, vec![next_anim]);
            }
        }

        anim_controller.anim_items.insert(deal_id, anim_item);
    }

    anim_controller.start_anim();
}

pub fn animate_deal(id: RenderId) -> (RenderId, AnimItem) {
    let canvas = unsafe { CANVAS.as_mut().unwrap() };
    let render_items = &canvas.render_items;
    let deck_pos = &render_items[&RenderId::Deck];
    let target_pos = &render_items[&id];

    // which player this deal is actually for, captured now (while `id` -- the
    // exact hand slot this animation is filling -- is still known) rather than
    // inferred later from "whoever's turn it is when the callback fires,
    // inverted". That inference assumed no other turn could pass in the 300ms
    // between scheduling this animation and it completing, which doesn't hold in
    // general (eg. the AI's own reply, or another queued deal, landing inside
    // that window) -- get_current_player_num() would then report the wrong
    // player, silently dealing the card into the wrong hand and leaving the
    // other hand one card short of its intended replenishment
    let (id_key, _) = id.key_val();
    let dealt_to_player_1 = id_key == "p1";

    (
        RenderId::Deal,
        AnimItem {
            start: None,
            duration: 0.3,
            attributes: HashMap::from([
                (AnimAttribute::X, (deck_pos.x, target_pos.x)),
                (AnimAttribute::Y, (deck_pos.y, target_pos.y)),
                (AnimAttribute::W, (deck_pos.w, target_pos.w)),
                (AnimAttribute::H, (deck_pos.h, target_pos.h)),
                (AnimAttribute::R, (deck_pos.r, target_pos.r)),
            ]),
            callback: vec![Box::new(move || {
                let game = unsafe { GAME.as_mut().unwrap() };
                let player = if dealt_to_player_1 {
                    &mut game.player_1
                } else {
                    &mut game.player_2
                };
                player.push(game.deck.pop().unwrap());
                render::draw();
            })],
        },
    )
}

pub struct AnimController {
    pub anim_items: HashMap<RenderId, AnimItem>, // map of currently animated items
    pub anim_chain: HashMap<RenderId, Vec<AnimItem>>, // map of chain animation callbacks
    pub render_animation_frame_handle: AnimationFrame, // current raf handle
}

impl AnimController {
    /// starts requestAnimationFrame callback
    pub fn start_anim(&mut self) {
        self.render_animation_frame_handle = request_animation_frame(on_animation_frame);
    }
}

/// generic animation item container
#[derive(Default)]
pub struct AnimItem {
    pub start: Option<f64>, // beginning timestamp of animation
    pub duration: f64,      // duration of animation in seconds
    pub attributes: HashMap<AnimAttribute, (f64, f64)>, // (start, end)
    // Box<dyn Fn()>, not a plain fn() -- animate_deal's callback needs to capture
    // which player it's dealing to (see its doc comment for why inferring that
    // from "current player, inverted" at the time the callback actually fires
    // was unsound), and a plain fn pointer can't capture anything. Neither Clone
    // nor Debug are derivable for a trait object like this (and neither was ever
    // actually used for AnimItem), so this only implements Default now.
    pub callback: Vec<Box<dyn Fn()>>, // callback list for animation end
}

/// attributes of a render item that are able to be interpolated
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum AnimAttribute {
    X, // x position of animated component
    Y, // y position of animated component
    W, // width of animated component
    H, // height of animated component
    R, // border radius of animated component
}

impl Display for AnimAttribute {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            AnimAttribute::X => write!(f, "X"),
            AnimAttribute::Y => write!(f, "Y"),
            AnimAttribute::W => write!(f, "W"),
            AnimAttribute::H => write!(f, "H"),
            AnimAttribute::R => write!(f, "R"),
        }
    }
}
