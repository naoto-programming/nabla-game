/// std imports
use rand::seq::SliceRandom;
use rand::thread_rng;
// outer crate imports
use crate::render::render;
use crate::render::util::RenderId;
// root imports
use crate::MENU;
// local imports
use super::card_counts::get_new_deck;
use super::cards::*;
use super::field::*;

// helper function to shuffle deck and deal cards to player hands
fn create_players(deck: &mut Vec<Card>) -> (Vec<Card>, Vec<Card>) {
    // deal initial hands
    let mut hand_1 = deck.split_off(deck.len() - 7);
    let mut hand_2 = deck.split_off(deck.len() - 7);

    // check for edge case where a player receives all basis cards
    while hand_1.iter().all(|card| card.card_type() == "BASIS_CARD")
        && hand_2.iter().all(|card| card.card_type() == "BASIS_CARD")
    {
        // mulligan and re shuffle
        deck.append(&mut hand_1);
        deck.append(&mut hand_2);
        deck.shuffle(&mut thread_rng());
        hand_1 = deck.split_off(deck.len() - 7);
        hand_2 = deck.split_off(deck.len() - 7);
    }

    (hand_1, hand_2)
}

/// main Game struct, holds all game state
#[derive(Debug)]
pub struct Game {
    pub state: GameState,
    pub turn: Turn, // turn counter
    pub field: Field,
    pub player_1: Vec<Card>, // up to 7 cards in hand (<7 if deck running low)
    pub player_2: Vec<Card>,
    pub deck: Vec<Card>,
    pub graveyard: Vec<Card>,
    pub active: ActiveCards,
    pub pending: Option<PendingAction>, // awaiting confirmation, see CONFIRM_BEFORE_PLAY
}

impl Game {
    /// create new game for start
    pub fn new() -> Game {
        let mut deck = get_new_deck();
        deck.shuffle(&mut thread_rng());

        let (player_1, player_2) = create_players(&mut deck);
        return Game {
            state: GameState::MENU,
            turn: Turn {
                number: 0,
                phase: TurnPhase::IDLE,
            },
            field: Field::new(),
            player_1: player_1,
            player_2: player_2,
            deck: deck,
            graveyard: vec![],
            active: ActiveCards {
                selected: Vec::default(),
                hover: None,
            },
            pending: None,
        };
    }

    /// get current player based on turn number
    pub fn get_current_player_num(&self) -> u32 {
        if self.turn.number % 2 == 0 {
            1
        } else {
            2
        }
    }
    /// get reference to current player hand
    pub fn get_current_player(&self) -> &Vec<Card> {
        match self.get_current_player_num() {
            1 => &self.player_1,
            2 => &self.player_2,
            _ => unreachable!("No Active Player"),
        }
    }

    /// set new game state
    pub fn set_state(&mut self, state: GameState) {
        let menu = unsafe { MENU.as_ref().unwrap() };

        self.state = state;
        match self.state {
            GameState::PLAYAI | GameState::PLAYVS => {
                menu.close();
                render::draw();
            }
            GameState::MENU => {
                menu.open();
            }
            _ => {}
        }
    }

    /// handle losing state
    pub fn game_over(&self, winner: u32) {
        let menu = unsafe { MENU.as_ref().unwrap() };

        menu.game_over_menu
            .get_elements_by_tag_name("h2")
            .item(0)
            .unwrap()
            .set_text_content(Some(format!("Player {} wins!", winner).as_str()));
        menu.open();
        menu.activate("GAMEOVER".to_string());
    }
}

/// turn struct to manager turn number and turn phases
#[derive(Debug)]
pub struct Turn {
    pub number: u32,
    pub phase: TurnPhase,
}

/// turn phases for the steps required in various cards
#[allow(non_camel_case_types)]
#[derive(Debug)]
pub enum TurnPhase {
    IDLE,               // start of turn
    SELECT(Card),       // single-basis operators or playing new operators with a blank slot
    FIELD_SELECT(Card), // nabla or laplacian
    MULTISELECT(Card),  // mult or div
    CONFIRM,            // previewing the resulting field, see CONFIRM_BEFORE_PLAY
}

/// a computed move waiting on player confirmation before it replaces `Game::field`
#[derive(Debug)]
pub struct PendingAction {
    pub field: Field,
    pub changed_indices: Vec<usize>,
}

/// struct to store currently selected cards
#[derive(Debug)]
pub struct ActiveCards {
    pub selected: Vec<RenderId>,
    pub hover: Option<RenderId>,
}

impl ActiveCards {
    pub fn clear(&mut self) {
        self.selected = Vec::default();
        self.hover = None;
    }
}

/// different possible states of game and UI
#[derive(Debug)]
pub enum GameState {
    MENU,
    PLAYAI,
    PLAYVS,
    PLAYONLINE,
    TUTORIAL,
    SETTINGS,
    CREDITS,
}

impl From<&str> for GameState {
    fn from(input: &str) -> Self {
        match input {
            "MENU" => Self::MENU,
            "PLAYAI" => Self::PLAYAI,
            "PLAYVS" => Self::PLAYVS,
            "PLAYONLINE" => Self::PLAYONLINE,
            "TUTORIAL" => Self::TUTORIAL,
            "SETTINGS" => Self::SETTINGS,
            "CREDITS" => Self::CREDITS,
            _ => unreachable!("{} is not a valid GameState", input),
        }
    }
}
