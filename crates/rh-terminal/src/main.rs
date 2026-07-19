//! Native terminal ASCII client: Bevy + Ratatui over the shared client core.
//!
//! A thin renderer: raw crossterm input becomes `Intent`s, the shared
//! `ClientSession` owns all behaviour, and each frame draws the viewmodel.
//! Active runs persist as share-code save files.

mod render;

use std::path::PathBuf;
use std::time::Duration;

use bevy::app::{App, AppExit, ScheduleRunnerPlugin};
use bevy::prelude::*;
use bevy_ratatui::event::{KeyMessage, MouseMessage, PasteMessage};
use bevy_ratatui::{RatatuiContext, RatatuiPlugins};
use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use rh_client_core::{ClientSession, Intent, Screen};
use rh_core::geometry::{Direction, Point};

#[derive(Resource)]
pub struct Client {
    pub session: ClientSession,
    pub save_path: PathBuf,
    /// Interactive regions from the last frame, for mouse hit-testing.
    pub areas: render::RunAreas,
}

fn main() {
    let catalogue = match rh_content::load_embedded() {
        Ok(catalogue) => catalogue,
        Err(error) => {
            eprintln!("content failed to validate: {error}");
            std::process::exit(1);
        }
    };
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(1);
    let mut session = ClientSession::new(catalogue, nonce);

    let save_path = save_path();
    if let Ok(code) = std::fs::read_to_string(&save_path) {
        session.restore(code.trim());
    }

    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16))))
        .add_plugins(RatatuiPlugins {
            enable_mouse_capture: true,
            ..Default::default()
        })
        .insert_resource(Client {
            session,
            save_path,
            areas: render::RunAreas::default(),
        })
        .add_systems(Update, (input_system, draw_system).chain())
        .run();
}

fn save_path() -> PathBuf {
    let mut base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.push("rogue-hunter");
    let _ = std::fs::create_dir_all(&base);
    base.push("active-run.rh1");
    base
}

/// Persist or clear the active-run save after every state change.
fn persist(client: &Client) {
    match (&client.session.screen, client.session.share_code()) {
        (Screen::Run, Some(code))
        | (Screen::Grimoire { .. }, Some(code))
        | (Screen::Relationships { .. }, Some(code))
        | (Screen::RegionMap { .. }, Some(code))
        | (Screen::EventLog { .. }, Some(code))
        | (Screen::Dossier { .. }, Some(code)) => {
            let _ = std::fs::write(&client.save_path, code);
        }
        (Screen::CaseReport, _) | (Screen::Splash { .. }, _) => {
            let _ = std::fs::remove_file(&client.save_path);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn input_system(
    mut client: ResMut<Client>,
    mut keys: MessageReader<KeyMessage>,
    mut mice: MessageReader<MouseMessage>,
    mut pastes: MessageReader<PasteMessage>,
    mut exit: MessageWriter<AppExit>,
) {
    let mut changed = false;
    for key in keys.read() {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        // Quit from the splash screen; Ctrl+Q anywhere.
        let on_splash = matches!(client.session.screen, Screen::Splash { .. });
        if (key.code == KeyCode::Esc && on_splash && client.session.modal.is_none())
            || (key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            exit.write_default();
            return;
        }
        if let Some(intent) = translate_key(&client.session, key.code) {
            client.session.handle(intent);
            changed = true;
        }
    }
    for paste in pastes.read() {
        client.session.handle(Intent::Paste(paste.0.clone()));
        changed = true;
    }
    for mouse in mice.read() {
        let map = client.areas.map;
        let actions = client.areas.actions;
        let in_map = |column: u16, row: u16| -> Option<Point> {
            if column >= map.x
                && column < map.x + map.width
                && row >= map.y
                && row < map.y + map.height
            {
                Some(Point::new((column - map.x) as i16, (row - map.y) as i16))
            } else {
                None
            }
        };
        let in_actions = |column: u16, row: u16| -> Option<usize> {
            if column >= actions.x
                && column < actions.x + actions.width
                && row >= actions.y
                && row < actions.y + actions.height
            {
                Some((row - actions.y) as usize)
            } else {
                None
            }
        };
        let menu = client.areas.menu;
        let in_menu = |column: u16, row: u16| -> Option<usize> {
            if menu.width > 0
                && menu.height > 0
                && column >= menu.x
                && column < menu.x + menu.width
                && row >= menu.y
                && row < menu.y + menu.height
            {
                Some((row - menu.y) as usize)
            } else {
                None
            }
        };
        match mouse.kind {
            MouseEventKind::Moved => {
                // Over a menu the mouse moves the highlight, so confirming
                // does what the highlight shows. Over the map it is a look
                // cursor; anywhere else it clears.
                let intent = match (
                    in_menu(mouse.column, mouse.row),
                    in_map(mouse.column, mouse.row),
                ) {
                    (Some(row), _) => Intent::HoverRow(row),
                    (None, Some(point)) => Intent::Hover(point),
                    (None, None) => Intent::HoverClear,
                };
                client.session.handle(intent);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // A modal menu draws over the map, so it is tested first.
                if let Some(row) = in_menu(mouse.column, mouse.row) {
                    client.session.handle(Intent::Select(row));
                    changed = true;
                } else if let Some(point) = in_map(mouse.column, mouse.row) {
                    client.session.handle(Intent::Click(point));
                    changed = true;
                } else if let Some(row) = in_actions(mouse.column, mouse.row) {
                    client.session.handle(Intent::DoAction(row));
                    changed = true;
                }
            }
            _ => {}
        }
    }
    if changed {
        persist(&client);
    }
}

/// Map a key press to an intent, respecting text-entry screens.
fn translate_key(session: &ClientSession, code: KeyCode) -> Option<Intent> {
    let text_entry = matches!(
        session.screen,
        Screen::SeedEntry { .. } | Screen::CodeEntry { .. }
    );
    if text_entry {
        return match code {
            KeyCode::Char(c) => Some(Intent::Char(c)),
            KeyCode::Backspace => Some(Intent::Backspace),
            KeyCode::Enter => Some(Intent::Confirm),
            KeyCode::Esc => Some(Intent::Cancel),
            _ => None,
        };
    }
    // Menu-style screens want list navigation on arrows.
    let in_menu = session.modal.is_some() || !matches!(session.screen, Screen::Run);
    match code {
        KeyCode::Esc => Some(Intent::Cancel),
        KeyCode::Enter => Some(Intent::Confirm),
        // Tab sweeps the look cursor over everything in sight.
        KeyCode::Tab if !in_menu => Some(Intent::NextThreat),
        KeyCode::Up if in_menu => Some(Intent::Up),
        KeyCode::Down if in_menu => Some(Intent::Down),
        KeyCode::Up => Some(Intent::Move(Direction::North)),
        KeyCode::Down => Some(Intent::Move(Direction::South)),
        KeyCode::Left => Some(Intent::Move(Direction::West)),
        KeyCode::Right => Some(Intent::Move(Direction::East)),
        // Numpad diagonals (NumLock off sends these navigation codes).
        KeyCode::Home if !in_menu => Some(Intent::Move(Direction::NorthWest)),
        KeyCode::PageUp if !in_menu => Some(Intent::Move(Direction::NorthEast)),
        KeyCode::End if !in_menu => Some(Intent::Move(Direction::SouthWest)),
        KeyCode::PageDown if !in_menu => Some(Intent::Move(Direction::SouthEast)),
        KeyCode::Char(c) => translate_char(session, c, in_menu),
        _ => None,
    }
}

fn translate_char(_session: &ClientSession, c: char, in_menu: bool) -> Option<Intent> {
    // Numpad digits (NumLock on) are roguelike movement in the run screen.
    if !in_menu {
        if let Some(intent) = numpad_move(c) {
            return Some(intent);
        }
    }
    match c {
        'h' => Some(Intent::Move(Direction::West)),
        'j' if !in_menu => Some(Intent::Move(Direction::South)),
        'k' if !in_menu => Some(Intent::Move(Direction::North)),
        'j' => Some(Intent::Down),
        'k' => Some(Intent::Up),
        'l' => Some(Intent::Move(Direction::East)),
        'y' => Some(Intent::Move(Direction::NorthWest)),
        'u' => Some(Intent::Move(Direction::NorthEast)),
        'b' => Some(Intent::Move(Direction::SouthWest)),
        'n' => Some(Intent::Move(Direction::SouthEast)),
        '.' | ' ' => Some(Intent::Wait),
        'e' => Some(Intent::Interact),
        'f' => Some(Intent::Fire),
        'F' => Some(Intent::FireSilver),
        'a' => Some(Intent::Aim),
        'p' => Some(Intent::PowerAttack),
        's' => Some(Intent::Sprint),
        'x' => Some(Intent::SetSnare),
        'K' => Some(Intent::KillingBlow),
        'q' => Some(Intent::Draught),
        'c' => Some(Intent::Charm),
        'd' => Some(Intent::Dossier),
        'g' => Some(Intent::Grimoire),
        'r' => Some(Intent::Relationships),
        'v' => Some(Intent::RegionMap),
        'L' => Some(Intent::EventLog),
        ';' => Some(Intent::ToggleLook),
        _ => None,
    }
}

/// Roguelike numpad movement: 1-9 laid out like the keypad, 5 waits.
fn numpad_move(c: char) -> Option<Intent> {
    Some(match c {
        '1' => Intent::Move(Direction::SouthWest),
        '2' => Intent::Move(Direction::South),
        '3' => Intent::Move(Direction::SouthEast),
        '4' => Intent::Move(Direction::West),
        '5' => Intent::Wait,
        '6' => Intent::Move(Direction::East),
        '7' => Intent::Move(Direction::NorthWest),
        '8' => Intent::Move(Direction::North),
        '9' => Intent::Move(Direction::NorthEast),
        _ => return None,
    })
}

fn draw_system(mut context: ResMut<RatatuiContext>, mut client: ResMut<Client>) {
    let view = client.session.view();
    let mut areas = client.areas;
    let result = context.draw(|frame| {
        areas = render::draw(frame, &view);
    });
    if result.is_err() {
        // Terminal trouble: nothing sensible to do but keep running.
        return;
    }
    client.areas = areas;
}
