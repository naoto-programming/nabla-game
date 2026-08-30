// std imports
use std::collections::HashMap;
// wasm-bindgen imports
use gloo::events::EventListener;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement, HtmlSelectElement};
// outer crate imports
use crate::game::ai::{AiDifficulty, AI_DIFFICULTY};
use crate::game::card_counts::{reset_card_counts, set_card_count, CARD_COUNT_NAMES, DEFAULT_CARD_COUNTS};
use crate::game::flags::*;
use crate::game::online;
use crate::game::structs::{Game, GameState};
use crate::render::katex::clear_katex_element;
use crate::render::util::{PLAYER_1_COLOUR, PLAYER_2_COLOUR};
// root imports
use super::{GAME, MENU};

/// controller for the main menu and submenus
pub struct Menu {
    pub menu_children: HashMap<String, Element>,
    pub menu_element: Element,

    pub main_menu_button: Element,
    pub main_menu_listener: EventListener,
    pub game_over_menu: Element,
    pub game_over_listener: EventListener,
    pub copy_match_data_button: Element,
    pub copy_match_data_listener: EventListener,

    pub main_menu: MainMenu,
    pub settings_menu: SettingsMenu,
    pub online_menu: OnlineMenu,
}

impl Menu {
    /// extracts child elements from DOM and stores with id as key
    pub fn new(document: &Document) -> Self {
        let menu_element = document.get_element_by_id("menu").unwrap();
        let mut menu_children = HashMap::new();
        let menu_html_children = menu_element
            .get_elements_by_class_name("button-wrapper")
            .item(0)
            .unwrap()
            .children();
        for i in 0..menu_html_children.length() {
            let child = menu_html_children.item(i).unwrap();
            // split id from 'menu-ID'
            let child_id = child.id();
            let id_kvp = child_id.split("-").collect::<Vec<&str>>();
            if id_kvp[0] == "menu" {
                menu_children.insert(id_kvp[1].to_string(), child.dyn_into::<Element>().unwrap());
            }
        }

        let main_menu_button = document.get_element_by_id("button-MENU").unwrap();
        let main_menu_listener = EventListener::new(&main_menu_button, "click", |_e| {
            let (game, menu_ref) = unsafe { (GAME.as_mut().unwrap(), MENU.as_ref()) };
            // this is the only way back to the top-level menu from any panel, including
            // an active game -- always tear down any online session here so a peer the
            // player has walked away from (whether mid-match or still waiting to
            // connect) can't keep mutating GAME in the background via late messages
            online::leave_room();
            game.set_state(GameState::from("MENU"));

            if menu_ref.is_some() {
                menu_ref.unwrap().activate("MENU".to_string());
            }
        });

        let game_over_menu = document.get_element_by_id("menu-GAMEOVER").unwrap();
        let game_over_listener = EventListener::new(
            &document.get_element_by_id("gameover-RESTART").unwrap(),
            "click",
            |_e| {
                let menu_ref = unsafe { MENU.as_ref() };
                if menu_ref.is_some() {
                    menu_ref.unwrap().activate("MENU".to_string());
                    unsafe {
                        GAME = Some(Game::new());
                        // clear graveyard katex items
                        ["g=1", "g=2", "g=3"].iter().for_each(|id| {
                            clear_katex_element(format!("katex-item_{}", id));
                        })
                    }
                }
            },
        );

        let copy_match_data_button = document
            .get_element_by_id("gameover-COPY_MATCH_DATA")
            .unwrap();
        let copy_match_data_listener =
            EventListener::new(&copy_match_data_button, "click", |_e| {
                crate::game::match_log::copy_match_data_to_clipboard();
            });

        let main_menu = MainMenu::new(document);
        let settings_menu = SettingsMenu::new(document);
        let online_menu = OnlineMenu::new(document);

        Menu {
            menu_children,
            menu_element,
            main_menu_button,
            main_menu_listener,
            game_over_menu,
            game_over_listener,
            copy_match_data_button,
            copy_match_data_listener,
            main_menu,
            settings_menu,
            online_menu,
        }
    }

    /// activate specific submenu, deactivate all others
    pub fn activate(&self, id: String) {
        for (element_id, element) in self.menu_children.iter() {
            if element_id == &id {
                element.remove_attribute("hidden").expect(
                    format!("Failed to hide {:?} with id {}", element, element_id).as_str(),
                );
            } else {
                element.set_attribute("hidden", "true").expect(
                    format!("Failed to show {:?} with id {}", element, element_id).as_str(),
                );
            }
        }
    }

    /// hide main menu and show game
    pub fn close(&self) {
        self.menu_element
            .set_attribute("hidden", "true")
            .expect("Failed to hide main menu");
    }

    /// show main menu and hide game
    pub fn open(&self) {
        self.menu_element
            .remove_attribute("hidden")
            .expect("Failed to show main menu");
    }
}

/// controller for the main menu
pub struct MainMenu {
    pub button_elements: Vec<Element>,
    pub button_listeners: HashMap<String, EventListener>,
}

impl MainMenu {
    /// extracts child elements from DOM and adds event listeners for each button
    pub fn new(document: &Document) -> Self {
        let button_elements: Vec<Element> =
            vec!["PLAYVS", "PLAYAI", "PLAYONLINE", "TUTORIAL", "SETTINGS", "CREDITS"]
                .iter()
                .map(|state| {
                    document
                        .get_element_by_id(&format!("button-{}", state).to_owned()[..])
                        .unwrap()
                })
                .collect();

        let mut button_listeners: HashMap<String, EventListener> = HashMap::new();
        for element in button_elements.iter() {
            let element_target = element.dyn_ref::<Element>().unwrap();
            let target_id = element_target.id();
            let listener = EventListener::new(element, "click", move |_e| {
                let (game, menu_ref) = unsafe { (GAME.as_mut().unwrap(), MENU.as_ref()) };
                // split id from 'button-ID'
                let target_state = target_id.split("-").nth(1).unwrap();
                game.set_state(GameState::from(target_state));

                if menu_ref.is_some() {
                    let menu = menu_ref.unwrap();
                    if menu.menu_children.contains_key(target_state) {
                        menu.activate(target_state.to_string());
                    }
                }
            });
            button_listeners.insert(element_target.id(), listener);
        }

        Self {
            button_elements,
            button_listeners,
        }
    }
}

/// controller for the settings menu
#[allow(dead_code)]
pub struct SettingsMenu {
    checkboxes: Vec<Element>,
    checkbox_listeners: HashMap<String, EventListener>,

    colours: Vec<Element>,
    colour_listeners: HashMap<String, EventListener>,

    ai_difficulty: Element,
    ai_difficulty_listener: EventListener,

    card_counts_button: Element,
    card_counts_button_listener: EventListener,
    card_counts: Vec<Element>,
    card_count_listeners: HashMap<String, EventListener>,
    reset_card_counts_button: Element,
    reset_card_counts_listener: EventListener,
}

impl SettingsMenu {
    /// extracts child elements from DOM and adds event listeners for each checkbox/select
    pub fn new(document: &Document) -> Self {
        let checkboxes: Vec<Element> = vec![
            "DISPLAY_LN_FOR_LOG",
            "ALLOW_LINEAR_DEPENDENCE",
            "FULL_COMPUTE",
            "USE_FRACTIONAL_EXPONENTS",
            "LIMIT_FIELD_BASIS",
            "CONFIRM_BEFORE_PLAY",
            "SHOW_MOVE_LOG",
        ]
        .iter()
        .map(|state| {
            document
                .get_element_by_id(format!("checkbox-{}", state).as_str())
                .unwrap()
        })
        .collect();

        let mut checkbox_listeners: HashMap<String, EventListener> = HashMap::new();
        // fetched once, up front, for the same reason as principal_label_element below
        let move_log_panel = document.get_element_by_id("move-log");
        for element in checkboxes.iter() {
            let move_log_panel = move_log_panel.clone();
            let listener = EventListener::new(element, "change", move |e| {
                let event_target = e.target().unwrap();
                let event_target_element = event_target.dyn_ref::<HtmlInputElement>().unwrap();

                // split id from 'checkbox-FLAG'
                let target_id = event_target_element.id();
                let flag_name = target_id.split("-").nth(1).unwrap();
                let flag_value = event_target_element.checked();

                unsafe {
                    match flag_name {
                        "DISPLAY_LN_FOR_LOG" => DISPLAY_LN_FOR_LOG = flag_value,
                        "ALLOW_LINEAR_DEPENDENCE" => ALLOW_LINEAR_DEPENDENCE = flag_value,
                        "FULL_COMPUTE" => FULL_COMPUTE = flag_value,
                        "USE_FRACTIONAL_EXPONENTS" => USE_FRACTIONAL_EXPONENTS = flag_value,
                        "LIMIT_FIELD_BASIS" => LIMIT_FIELD_BASIS = flag_value,
                        "CONFIRM_BEFORE_PLAY" => CONFIRM_BEFORE_PLAY = flag_value,
                        "SHOW_MOVE_LOG" => {
                            SHOW_MOVE_LOG = flag_value;
                            // hides the panel outright when off, rather than just
                            // gating future entries -- otherwise turning it back on
                            // mid-match would resurface stale entries from before
                            // it was switched off
                            if let Some(panel) = &move_log_panel {
                                if flag_value {
                                    panel.remove_attribute("hidden").ok();
                                } else {
                                    panel.set_attribute("hidden", "true").ok();
                                }
                            }
                        }
                        _ => panic!("Unknown flag name: {}", flag_name),
                    }
                }
            });
            checkbox_listeners.insert(element.id(), listener);
        }

        // Handle ALLOW_LIMITS_BEYOND_BOUNDS select
        let limits_select = document
            .get_element_by_id("select-ALLOW_LIMITS_BEYOND_BOUNDS")
            .unwrap();
        // fetched once, up front -- the closure below is `move` and stored in the
        // returned SettingsMenu (long-lived), so it can't hold a borrow of
        // `document` itself (only valid for this constructor call); the specific
        // element it needs is captured instead
        let principal_label_element = document.get_element_by_id("label-INVERSE_TRIG_PRINCIPAL_VALUE");
        let limits_listener = EventListener::new(&limits_select, "change", move |e| {
            let event_target = e.target().unwrap();
            let event_target_element = event_target.dyn_ref::<HtmlSelectElement>().unwrap();
            let value = event_target_element.value();
            let mode: u8 = value.parse().unwrap_or(1);

            unsafe {
                ALLOW_LIMITS_BEYOND_BOUNDS = mode;
            }

            // Show/hide principal value selection based on mode
            if let Some(label) = &principal_label_element {
                if mode == 2 {
                    label.remove_attribute("hidden").ok();
                } else {
                    label.set_attribute("hidden", "true").ok();
                }
            }
        });
        checkbox_listeners.insert(limits_select.id(), limits_listener);

        // Handle INVERSE_TRIG_PRINCIPAL_VALUE select
        let principal_select = document
            .get_element_by_id("select-INVERSE_TRIG_PRINCIPAL_VALUE");
        if let Some(select) = principal_select {
            let principal_listener = EventListener::new(&select, "change", move |e| {
                let event_target = e.target().unwrap();
                let event_target_element = event_target.dyn_ref::<HtmlSelectElement>().unwrap();
                let value = event_target_element.value();
                let principal: u8 = value.parse().unwrap_or(0);
                
                unsafe {
                    INVERSE_TRIG_PRINCIPAL_VALUE = principal;
                }
            });
            checkbox_listeners.insert(select.id(), principal_listener);
        }

        let colours: Vec<Element> = vec!["PLAYER_1", "PLAYER_2"]
            .iter()
            .map(|state| {
                document
                    .get_element_by_id(format!("colour-{}", state).as_str())
                    .unwrap()
            })
            .collect();

        let mut colour_listeners: HashMap<String, EventListener> = HashMap::new();
        for i in 0..2 {
            let player_target = colours[i].dyn_ref::<Element>().unwrap();
            let listener = EventListener::new(&colours[i], "change", move |e| {
                let event_target = e.target().unwrap();
                let player_colour = event_target.dyn_ref::<HtmlInputElement>().unwrap().value();
                unsafe {
                    if i == 0 {
                        PLAYER_1_COLOUR = Box::leak(player_colour.clone().into_boxed_str());
                    } else {
                        PLAYER_2_COLOUR = Box::leak(player_colour.clone().into_boxed_str());
                    }
                }
            });
            colour_listeners.insert(player_target.id(), listener);
        }

        let ai_difficulty = document
            .get_element_by_id("select-AI_DIFFICULTY")
            .unwrap();
        let ai_difficulty_listener = EventListener::new(&ai_difficulty, "change", |e| {
            let event_target = e.target().unwrap();
            let value = event_target.dyn_ref::<HtmlSelectElement>().unwrap().value();
            unsafe {
                AI_DIFFICULTY = AiDifficulty::from(value.as_str());
            }
        });

        // opens the card-count panel from within Settings (it lives alongside the
        // other top-level menu items so Menu::activate can show/hide it the same way)
        let card_counts_button = document.get_element_by_id("button-CARDCOUNTS").unwrap();
        let card_counts_button_listener =
            EventListener::new(&card_counts_button, "click", |_e| {
                let menu_ref = unsafe { MENU.as_ref() };
                if let Some(menu) = menu_ref {
                    menu.activate("CARDCOUNTS".to_string());
                }
            });

        let card_counts: Vec<Element> = CARD_COUNT_NAMES
            .iter()
            .map(|name| {
                document
                    .get_element_by_id(format!("count-{}", name).as_str())
                    .unwrap()
            })
            .collect();

        let mut card_count_listeners: HashMap<String, EventListener> = HashMap::new();
        for element in card_counts.iter() {
            let listener = EventListener::new(element, "change", |e| {
                let event_target = e.target().unwrap();
                let input = event_target.dyn_ref::<HtmlInputElement>().unwrap();

                // split id from 'count-NAME'
                let target_id = input.id();
                let count_name = target_id.split_once("-").unwrap().1.to_string();
                let value: u32 = input.value().parse().unwrap_or(0);
                set_card_count(count_name.as_str(), value);
            });
            card_count_listeners.insert(element.id(), listener);
        }

        let reset_card_counts_button = document
            .get_element_by_id("button-RESET_CARD_COUNTS")
            .unwrap();
        let reset_inputs = card_counts.clone();
        let reset_card_counts_listener =
            EventListener::new(&reset_card_counts_button, "click", move |_e| {
                reset_card_counts();
                for (element, default) in reset_inputs.iter().zip(DEFAULT_CARD_COUNTS.iter()) {
                    element
                        .dyn_ref::<HtmlInputElement>()
                        .unwrap()
                        .set_value(default.to_string().as_str());
                }
            });

        Self {
            checkboxes,
            checkbox_listeners,
            colours,
            colour_listeners,
            ai_difficulty,
            ai_difficulty_listener,
            card_counts_button,
            card_counts_button_listener,
            card_counts,
            card_count_listeners,
            reset_card_counts_button,
            reset_card_counts_listener,
        }
    }
}

/// controller for the "Play Online" create/join panel
#[allow(dead_code)]
pub struct OnlineMenu {
    create_button: Element,
    create_listener: EventListener,
    join_show_button: Element,
    join_show_listener: EventListener,
    join_connect_button: Element,
    join_connect_listener: EventListener,
    copy_code_button: Element,
    copy_code_listener: EventListener,
    copy_link_button: Element,
    copy_link_listener: EventListener,
}

impl OnlineMenu {
    pub fn new(document: &Document) -> Self {
        let create_panel = document.get_element_by_id("online-create-panel").unwrap();
        let join_panel = document.get_element_by_id("online-join-panel").unwrap();
        let room_code_display = document.get_element_by_id("online-room-code").unwrap();
        let status = document.get_element_by_id("online-status").unwrap();
        let join_input = document
            .get_element_by_id("online-join-code-input")
            .unwrap();

        let create_button = document.get_element_by_id("button-ONLINE_CREATE").unwrap();
        let create_listener = {
            let create_panel = create_panel.clone();
            // hidden if the player opens "Join Game" first, then switches to
            // "Create Game" without connecting -- otherwise both panels (one
            // with a stale join code input, the other with the new room code)
            // would show at once
            let join_panel = join_panel.clone();
            let room_code_display = room_code_display.clone();
            let status = status.clone();
            EventListener::new(&create_button, "click", move |_e| {
                let code = online::create_room();
                room_code_display.set_text_content(Some(code.as_str()));
                join_panel.set_attribute("hidden", "true").ok();
                create_panel.remove_attribute("hidden").ok();
                status.set_text_content(Some("Waiting for opponent..."));
            })
        };

        let join_show_button = document
            .get_element_by_id("button-ONLINE_JOIN_SHOW")
            .unwrap();
        let join_show_listener = {
            let join_panel = join_panel.clone();
            // see create_listener's matching comment -- same reasoning, other direction
            let create_panel = create_panel.clone();
            let status = status.clone();
            EventListener::new(&join_show_button, "click", move |_e| {
                create_panel.set_attribute("hidden", "true").ok();
                join_panel.remove_attribute("hidden").ok();
                status.set_text_content(Some(""));
            })
        };

        let join_connect_button = document
            .get_element_by_id("button-ONLINE_JOIN_CONNECT")
            .unwrap();
        let join_connect_listener = {
            let join_input = join_input.clone();
            let status = status.clone();
            EventListener::new(&join_connect_button, "click", move |_e| {
                let code = join_input.dyn_ref::<HtmlInputElement>().unwrap().value();
                online::join_room(code);
                status.set_text_content(Some("Connecting..."));
            })
        };

        let copy_code_button = document.get_element_by_id("button-COPY_CODE").unwrap();
        let copy_code_listener = {
            let room_code_display = room_code_display.clone();
            EventListener::new(&copy_code_button, "click", move |_e| {
                if let Some(code) = room_code_display.text_content() {
                    online::copy_to_clipboard(code);
                }
            })
        };

        let copy_link_button = document.get_element_by_id("button-COPY_LINK").unwrap();
        let copy_link_listener = {
            let room_code_display = room_code_display.clone();
            EventListener::new(&copy_link_button, "click", move |_e| {
                if let Some(code) = room_code_display.text_content() {
                    let location = web_sys::window().unwrap().location();
                    let url = format!(
                        "{}{}?room={}",
                        location.origin().unwrap(),
                        location.pathname().unwrap(),
                        code
                    );
                    online::copy_to_clipboard(url);
                }
            })
        };

        // if the page was opened via a ?room=CODE link, jump straight to the
        // join panel with the code pre-filled
        if let Some(code) = online::room_code_from_url() {
            join_input
                .dyn_ref::<HtmlInputElement>()
                .unwrap()
                .set_value(code.as_str());
            join_panel.remove_attribute("hidden").ok();
        }

        Self {
            create_button,
            create_listener,
            join_show_button,
            join_show_listener,
            join_connect_button,
            join_connect_listener,
            copy_code_button,
            copy_code_listener,
            copy_link_button,
            copy_link_listener,
        }
    }
}
