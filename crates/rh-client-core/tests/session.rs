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
    assert!(matches!(client.screen, Screen::RegionMap));
    client.handle(Intent::Cancel);

    // Sprint is modal: two directions.
    client.handle(Intent::Sprint);
    assert!(client.modal.is_some());
    client.handle(Intent::Cancel);
    assert!(client.modal.is_none());

    // Commands are being recorded for the replay save.
    assert!(client.share_code().expect("active run").starts_with("RH1-"));
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
