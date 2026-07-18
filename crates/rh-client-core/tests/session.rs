//! Drive the shared client session the way a user would: splash, seed
//! entry, movement, interaction menus, overlays, and the case report.

use rh_client_core::{ClientSession, Intent, Screen};
use rh_core::geometry::Direction;

fn session() -> ClientSession {
    let catalogue = rh_content::load_embedded().expect("embedded content");
    ClientSession::new(catalogue, 12345)
}

/// Drive the splash the way a player does, all the way into a run: pick
/// "Enter Seed", type one, then choose the hunter offered first.
fn run_on_seed(seed: &str) -> ClientSession {
    let mut client = session();
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    for digit in seed.chars() {
        client.handle(Intent::Char(digit));
    }
    client.handle(Intent::Confirm);
    client.handle(Intent::Confirm);
    client
}

#[test]
fn splash_menu_starts_a_run_by_seed() {
    let mut client = session();
    assert!(matches!(client.screen, Screen::Splash { .. }));

    // Navigate to "Enter Seed" and type one.
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    assert!(matches!(client.screen, Screen::SeedEntry { .. }));
    for digit in "11".chars() {
        client.handle(Intent::Char(digit));
    }
    client.handle(Intent::Confirm);
    // The world is certified for a particular hunter, so the seed is not
    // enough on its own: who takes the case is asked before it is generated.
    assert!(matches!(client.screen, Screen::HunterSelect { .. }));
    client.handle(Intent::Confirm);
    assert!(matches!(client.screen, Screen::Run), "run should begin");
    assert!(client.run.is_some());

    // The viewmodel renders a populated frame.
    let view = client.view();
    match view.screen {
        rh_client_core::view::ScreenView::Run(run) => {
            assert!(!run.header.is_empty());
            assert!(run.cells.iter().any(|cell| cell.glyph == '@'));
        }
        other => panic!("expected run view, got {other:?}"),
    }
}

#[test]
fn movement_and_menus_flow() {
    let mut client = run_on_seed("11");

    let before = client.run.as_ref().map(|run| run.sim.state.hunter.pos);
    // Two steps in the same direction cannot return to the start.
    client.handle(Intent::Move(Direction::East));
    client.handle(Intent::Move(Direction::East));
    let after = client.run.as_ref().map(|run| run.sim.state.hunter.pos);
    assert_ne!(before, after, "steps on open ground should land");

    // Screens open and close.
    client.handle(Intent::Grimoire);
    assert!(matches!(client.screen, Screen::Grimoire { .. }));
    client.handle(Intent::Cancel);
    assert!(matches!(client.screen, Screen::Run));
    client.handle(Intent::RegionMap);
    assert!(matches!(client.screen, Screen::RegionMap { .. }));
    // Arrow keys navigate the list; Down moves the selection.
    client.handle(Intent::Down);
    assert!(matches!(client.screen, Screen::RegionMap { selected } if selected == 1));
    client.handle(Intent::Cancel);
    assert!(matches!(client.screen, Screen::Run));

    // Sprint is modal: one direction, then it fires.
    client.handle(Intent::Sprint);
    assert!(client.modal.is_some());
    client.handle(Intent::Cancel);
    assert!(client.modal.is_none());

    // Commands are being recorded for the replay save.
    assert!(client.share_code().expect("active run").starts_with("RH1-"));
}

#[test]
fn look_mode_detaches_a_cursor_without_moving_the_hunter() {
    let mut client = run_on_seed("11");

    let hunter_before = client.run.as_ref().map(|run| run.sim.state.hunter.pos);

    // Enter look mode: a cursor detaches at the hunter's tile.
    client.handle(Intent::ToggleLook);
    assert!(client.is_looking(), "look mode should be active");
    let cursor_start = client.look_point();
    assert_eq!(cursor_start, hunter_before);

    // Moving in look mode moves the cursor, not the hunter.
    client.handle(Intent::Move(Direction::East));
    let hunter_after = client.run.as_ref().map(|run| run.sim.state.hunter.pos);
    assert_eq!(
        hunter_before, hunter_after,
        "hunter must not move while looking"
    );
    assert_ne!(
        client.look_point(),
        cursor_start,
        "cursor should have moved"
    );

    // The cursor tile inspects to a non-empty description.
    assert!(client.inspect(client.look_point().unwrap()).is_some());

    // Esc leaves look mode.
    client.handle(Intent::Cancel);
    assert!(!client.is_looking());
}

#[test]
fn action_panel_lists_keyed_actions_and_dispatches_clicks() {
    let mut client = run_on_seed("11");

    let actions = client.available_actions();
    assert!(!actions.is_empty(), "run should offer actions");
    // The look toggle is always present and keyed.
    let look = actions
        .iter()
        .position(|a| a.intent == Intent::ToggleLook)
        .expect("look action present");
    assert_eq!(actions[look].key, ";");

    // Clicking the look row (by index) toggles look mode.
    client.handle(Intent::DoAction(look));
    assert!(client.is_looking());
}

#[test]
fn record_groups_events_into_one_multiline_entry_per_day() {
    let mut client = run_on_seed("11");

    // Open The Record. It should list days, not individual events, and open
    // on the newest day.
    client.handle(Intent::EventLog);
    let view = client.view();
    match view.screen {
        rh_client_core::view::ScreenView::List {
            title,
            entries,
            selected,
        } => {
            assert_eq!(title, "The Record");
            assert!(!entries.is_empty());
            // Every heading names a day (or the final night); the body holds
            // that day's events, one per line.
            for (heading, body) in &entries {
                assert!(
                    heading.starts_with("Day ") || heading == "The final night",
                    "unexpected record heading: {heading}"
                );
                assert!(!body.is_empty());
            }
            // The first day already has several events on one entry.
            assert!(entries[0].1.contains('\n'), "day 0 should have many lines");
            assert_eq!(selected, Some(entries.len() - 1), "opens on the newest day");
        }
        other => panic!("expected record list, got {other:?}"),
    }

    // Arrow keys navigate without panicking; selection clamps in the view.
    client.handle(Intent::Up);
    client.handle(Intent::Down);
    assert!(matches!(client.screen, Screen::EventLog { .. }));
}

#[test]
fn restoring_a_share_code_resumes_the_run() {
    let mut client = run_on_seed("7");
    client.handle(Intent::Move(Direction::North));
    client.handle(Intent::Wait);
    let code = client.share_code().expect("active run");
    let digest = client.run.as_ref().map(|run| run.state_digest());

    let mut restored = session();
    assert!(restored.restore(&code), "share code should restore");
    assert_eq!(
        restored.run.as_ref().map(|run| run.state_digest()),
        digest,
        "restored run must match exactly"
    );
    assert!(matches!(restored.screen, Screen::Run));
}

#[test]
fn hunter_selection_lists_every_hunter_and_starts_as_the_chosen_one() {
    let mut client = session();
    let roster: Vec<String> = client.catalogue.hunters.keys().cloned().collect();
    assert!(
        roster.len() >= 2,
        "selection needs something to choose from"
    );

    // "New Run" asks who takes the case before generating anything.
    client.handle(Intent::Confirm);
    assert!(matches!(
        client.screen,
        Screen::HunterSelect { seed: None, .. }
    ));
    assert!(
        client.run.is_none(),
        "no world is built until a hunter is picked"
    );

    // Every hunter is offered, with what distinguishes her.
    match client.view().screen {
        rh_client_core::view::ScreenView::List { entries, .. } => {
            assert_eq!(entries.len(), roster.len());
            for (heading, body) in &entries {
                assert!(!heading.is_empty());
                assert!(
                    body.contains("Mystic"),
                    "the pools are what differ between hunters, so they must be shown"
                );
            }
        }
        other => panic!("expected a list, got {other:?}"),
    }

    // Pick the second hunter; the run must actually be hers.
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    assert!(matches!(client.screen, Screen::Run));
    let run = client.run.as_ref().expect("run started");
    assert_eq!(run.hunter, roster[1]);
    assert_eq!(run.sim.catalogue.hunter_id, roster[1]);
}

#[test]
fn a_chosen_hunter_survives_the_share_code_round_trip() {
    let mut client = session();
    client.handle(Intent::Confirm);
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    let chosen = client.run.as_ref().expect("run").hunter.clone();
    let code = client.share_code().expect("share code");

    let mut restored = session();
    assert!(restored.restore(&code), "share code should restore");
    assert_eq!(
        restored.run.as_ref().expect("restored run").hunter,
        chosen,
        "a replay must be played by the hunter it was recorded for"
    );
}

#[test]
fn cancelling_hunter_selection_returns_to_the_splash() {
    let mut client = session();
    client.handle(Intent::Confirm);
    assert!(matches!(client.screen, Screen::HunterSelect { .. }));
    client.handle(Intent::Cancel);
    assert!(matches!(client.screen, Screen::Splash { .. }));
    assert!(client.run.is_none());
}

#[test]
#[ignore = "diagnostic: prints the hunter selection screen; run with --ignored"]
fn print_hunter_select_screen() {
    let mut client = session();
    client.handle(Intent::Confirm);
    match client.view().screen {
        rh_client_core::view::ScreenView::List {
            title,
            entries,
            selected,
        } => {
            println!("== {title} == (selected {selected:?})");
            for (index, (heading, body)) in entries.iter().enumerate() {
                let mark = if Some(index) == selected { ">" } else { " " };
                println!("{mark} {heading}");
                for line in body.lines() {
                    println!("      {line}");
                }
            }
        }
        other => panic!("expected list, got {other:?}"),
    }
}

#[test]
fn clicking_a_hunter_row_picks_that_hunter() {
    let mut client = session();
    let roster: Vec<String> = client.catalogue.hunters.keys().cloned().collect();
    client.handle(Intent::Confirm);
    assert!(matches!(client.screen, Screen::HunterSelect { .. }));

    // A click selects and activates in one go, without walking the list.
    client.handle(Intent::Select(1));
    assert!(matches!(client.screen, Screen::Run));
    assert_eq!(client.run.as_ref().expect("run").hunter, roster[1]);
}

#[test]
fn clicking_a_splash_option_activates_it() {
    let mut client = session();
    client.handle(Intent::Select(2));
    assert!(
        matches!(client.screen, Screen::CodeEntry { .. }),
        "the third option is Paste Replay Code"
    );
}

#[test]
fn clicking_past_the_end_of_a_menu_does_nothing() {
    let mut client = session();
    client.handle(Intent::Confirm);
    client.handle(Intent::Select(99));
    assert!(
        matches!(client.screen, Screen::HunterSelect { .. }),
        "an out-of-range click must not start a run or panic"
    );
    assert!(client.run.is_none());
}

#[test]
fn clicking_a_reference_list_moves_the_reading_position_only() {
    let mut client = run_on_seed("11");
    client.handle(Intent::Grimoire);
    client.handle(Intent::Select(2));
    assert!(
        matches!(client.screen, Screen::Grimoire { selected } if selected == 2),
        "a reference list has nothing to activate, so it should just scroll"
    );
}

#[test]
fn hovering_a_menu_row_highlights_without_choosing_it() {
    let mut client = session();
    client.handle(Intent::Confirm);

    // Hover moves the highlight only: nothing is generated yet.
    client.handle(Intent::HoverRow(1));
    assert!(
        matches!(client.screen, Screen::HunterSelect { selected: 1, .. }),
        "hover should move the highlight"
    );
    assert!(client.run.is_none(), "hovering must not start a run");

    // The detail pane follows the pointer.
    match client.view().screen {
        rh_client_core::view::ScreenView::List { selected, .. } => {
            assert_eq!(selected, Some(1));
        }
        other => panic!("expected list, got {other:?}"),
    }

    // Confirming now takes the hovered row, so mouse and keyboard agree.
    let roster: Vec<String> = client.catalogue.hunters.keys().cloned().collect();
    client.handle(Intent::Confirm);
    assert_eq!(client.run.as_ref().expect("run").hunter, roster[1]);
}

#[test]
fn hovering_past_the_end_of_a_menu_is_ignored() {
    let mut client = session();
    client.handle(Intent::Confirm);
    client.handle(Intent::HoverRow(99));
    assert!(matches!(
        client.screen,
        Screen::HunterSelect { selected: 0, .. }
    ));
}
