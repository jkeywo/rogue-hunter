//! Drive the shared client session the way a user would: splash, seed
//! entry, movement, interaction menus, overlays, and the case report.

use rh_client_core::{ClientSession, Intent, Screen};
use rh_core::geometry::Direction;

fn session() -> ClientSession {
    let catalogue = rh_content::load_embedded().expect("embedded content");
    ClientSession::new(catalogue, 12345)
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
    let mut client = session();
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    for digit in "11".chars() {
        client.handle(Intent::Char(digit));
    }
    client.handle(Intent::Confirm);

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
    let mut client = session();
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    for digit in "11".chars() {
        client.handle(Intent::Char(digit));
    }
    client.handle(Intent::Confirm);

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
    let mut client = session();
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    for digit in "11".chars() {
        client.handle(Intent::Char(digit));
    }
    client.handle(Intent::Confirm);

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
    let mut client = session();
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    for digit in "11".chars() {
        client.handle(Intent::Char(digit));
    }
    client.handle(Intent::Confirm);

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
    let mut client = session();
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    for digit in "7".chars() {
        client.handle(Intent::Char(digit));
    }
    client.handle(Intent::Confirm);
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
