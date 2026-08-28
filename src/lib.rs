use gloo::events::EventListener;
use wasm_bindgen::prelude::*;

pub mod basis;
pub mod game;
use game::structs::Game;
pub mod math;
mod menu;
use menu::*;

mod events;
use events::event_listeners::*;

mod canvas;
use canvas::*;
pub mod render;

mod util;

// When the `wee_alloc` feature is enabled, this uses `wee_alloc` as the global
// allocator.
//
// If you don't want to use `wee_alloc`, you can safely delete this.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

pub static mut CANVAS: Option<Canvas> = None;
pub static mut GAME: Option<Game> = None;
pub static mut MENU: Option<Menu> = None;

// This is like the `main` function, except for JavaScript.
#[wasm_bindgen(start)]
pub fn main_js() -> Result<(), JsValue> {
    // Turns an otherwise-opaque WASM trap ("unreachable executed", no further
    // detail) into the actual Rust panic message + location in the browser
    // console. This used to be debug-only (release "disabled it so it doesn't
    // bloat up the file size"), but that tradeoff means any panic that reaches
    // production is completely unreadable -- exactly the information needed to
    // diagnose remaining AI-freeze reports. The size cost is a few KB, worth
    // paying for being able to see what actually panicked in the field.
    console_error_panic_hook::set_once();

    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    unsafe {
        GAME = Some(Game::new());
        CANVAS = Some(Canvas::new(&document));
        MENU = Some(Menu::new(&document));
    }
    game::learning::load_persisted_table();
    let canvas = unsafe { CANVAS.as_mut().unwrap() };

    canvas.mousedown_listener = Some(EventListener::new(
        &canvas.canvas_element,
        "mousedown",
        mousedown_event_listener,
    ));
    canvas.mousemove_listener = Some(EventListener::new(
        &canvas.canvas_element,
        "mousemove",
        mousemove_event_listener,
    ));

    canvas.resize();
    render::render::draw();

    EventListener::new(&window, "resize", |_e| {
        let canvas = unsafe { CANVAS.as_mut().unwrap() };
        canvas.resize();
        render::render::draw();
    })
    .forget();

    Ok(())
}
