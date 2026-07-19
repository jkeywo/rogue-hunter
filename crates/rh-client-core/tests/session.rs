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
            // Bracketed: the title is placeholder copy from the string table.
            assert_eq!(title, "[The Record]");
            assert!(!entries.is_empty());
            // Every heading names a day (or the final night); the body holds
            // that day's events, one per line.
            for (heading, body) in &entries {
                // Bracketed: still placeholder copy from the string table.
                assert!(
                    heading.starts_with("[Day ") || heading == "[The final night]",
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

/// Walk the hunter onto the first exit tile of the current map, the way a
/// player does: point at it and walk, picking the walk up again whenever
/// something interrupts it. Returns false if the ground could not be reached.
fn walk_onto_an_exit(client: &mut ClientSession) -> bool {
    let run = client.run.as_ref().expect("run");
    let exit = run.sim.world.map(run.sim.state.current_map).exits[0].at;
    for _ in 0..24 {
        let before = client.run.as_ref().expect("run").sim.state.hunter.pos;
        if before == exit {
            return true;
        }
        client.handle(Intent::TravelTo(exit));
        let after = client.run.as_ref().expect("run").sim.state.hunter.pos;
        if after == before {
            return false;
        }
    }
    false
}

#[test]
fn pointing_at_distant_ground_walks_the_whole_way_there() {
    let mut client = run_on_seed("11");
    let start = client.run.as_ref().expect("run").sim.state.hunter.pos;

    // Find open ground several tiles off and walk to it in one intent.
    let exit = {
        let run = client.run.as_ref().expect("run");
        run.sim.world.map(run.sim.state.current_map).exits[0].at
    };
    assert!(
        start.distance(exit) > 1,
        "the exit should be somewhere worth walking to"
    );
    client.handle(Intent::TravelTo(exit));

    let after = client.run.as_ref().expect("run").sim.state.hunter.pos;
    assert_ne!(after, start, "one walk intent should cover ground");
    assert!(
        start.distance(after) > 1 || after == exit,
        "a walk should be more than the single step a click used to take"
    );
    // Arriving clears the target; being interrupted keeps it, so the walk can
    // be picked up rather than re-aimed.
    if after == exit {
        assert_eq!(client.travel_target, None, "arrival clears the target");
    } else {
        assert_eq!(
            client.travel_target,
            Some(exit),
            "an interrupted walk remembers where it was going"
        );
    }
}

#[test]
fn an_interrupted_walk_is_offered_again_rather_than_re_aimed() {
    let mut client = run_on_seed("11");
    let exit = {
        let run = client.run.as_ref().expect("run");
        run.sim.world.map(run.sim.state.current_map).exits[0].at
    };
    client.handle(Intent::TravelTo(exit));
    if client.travel_target.is_none() {
        // It arrived first go; nothing to resume, which is the other branch.
        return;
    }
    // With nothing under the cursor, the walk row offers to pick it up again.
    let resume = client
        .available_actions()
        .into_iter()
        .find(|action| action.intent == Intent::TravelTo(exit))
        .expect("the interrupted walk should be offered again");
    assert!(resume.enabled);
}

#[test]
fn tab_walks_the_cursor_over_what_is_in_sight() {
    // Find a seed that opens with something visible, so the assertion is
    // about the cycling rather than about the seed.
    let mut client = (1..40u32)
        .map(|seed| run_on_seed(&seed.to_string()))
        .find(|client| !client.in_sight().is_empty())
        .expect("some opening has something in sight");

    let sighted = client.in_sight();
    client.handle(Intent::NextThreat);
    assert_eq!(
        client.look_point(),
        Some(sighted[0].at),
        "the first press lands on the nearest thing in sight"
    );

    // Pressing again moves on, wrapping back round to the start.
    for step in 1..=sighted.len() {
        client.handle(Intent::NextThreat);
        assert_eq!(client.look_point(), Some(sighted[step % sighted.len()].at));
    }
}

#[test]
fn nothing_in_sight_says_so_rather_than_moving_the_cursor() {
    let mut client = (1..40u32)
        .map(|seed| run_on_seed(&seed.to_string()))
        .find(|client| client.in_sight().is_empty())
        .expect("some opening has nothing in sight");
    client.handle(Intent::NextThreat);
    assert_eq!(client.look_point(), None);
    assert!(!client.status.is_empty(), "it should say why nothing moved");
}

#[test]
fn travel_is_asked_for_before_it_spends_a_day() {
    let mut client = run_on_seed("11");
    assert!(
        walk_onto_an_exit(&mut client),
        "the exit should be walkable"
    );
    let day_before = client.run.as_ref().expect("run").sim.state.clock;

    // The road out is offered by name; taking it opens a gate rather than
    // spending the day on the spot.
    client.handle(Intent::Interact);
    if let Some(rh_client_core::Modal::Menu { items, .. }) = client.modal.clone() {
        let travel = items
            .iter()
            .position(|item| {
                matches!(
                    item.action,
                    rh_client_core::MenuAction::Do(rh_core::command::Command::Travel)
                )
            })
            .expect("standing on an exit offers the road");
        client.handle(Intent::Select(travel));
    }
    assert!(
        matches!(client.modal, Some(rh_client_core::Modal::Confirm { .. })),
        "travel should ask first, got {:?}",
        client.modal
    );
    assert_eq!(
        client.run.as_ref().expect("run").sim.state.clock,
        day_before,
        "asking must not have spent the day"
    );

    // Backing out leaves the clock alone.
    client.handle(Intent::Cancel);
    assert!(client.modal.is_none());
    assert_eq!(
        client.run.as_ref().expect("run").sim.state.clock,
        day_before
    );

    // Going ahead spends it.
    client.handle(Intent::Interact);
    if matches!(client.modal, Some(rh_client_core::Modal::Menu { .. })) {
        let items = match client.modal.clone() {
            Some(rh_client_core::Modal::Menu { items, .. }) => items,
            _ => unreachable!(),
        };
        let travel = items
            .iter()
            .position(|item| {
                matches!(
                    item.action,
                    rh_client_core::MenuAction::Do(rh_core::command::Command::Travel)
                )
            })
            .expect("the road is still offered");
        client.handle(Intent::Select(travel));
    }
    client.handle(Intent::Confirm);
    assert!(client.modal.is_none());
    assert_eq!(
        client.run.as_ref().expect("run").sim.state.clock,
        day_before + 1,
        "confirming should spend the day"
    );
}

#[test]
fn the_dossier_synthesises_what_is_known() {
    let mut client = run_on_seed("11");
    client.handle(Intent::Dossier);
    assert!(matches!(client.screen, Screen::Dossier { .. }));

    match client.view().screen {
        rh_client_core::view::ScreenView::List { title, entries, .. } => {
            // Bracketed: placeholder copy from the string table.
            assert_eq!(title, "[The Case So Far]");
            assert_eq!(entries.len(), 3, "the quarry, the leads, the preparations");
            for (heading, body) in &entries {
                assert!(heading.starts_with('['), "unbracketed heading: {heading}");
                assert!(!body.is_empty(), "{heading} should say something");
            }
            // The quarry section is the one that carries the clock and the
            // naming test, because that is what a player is asking about.
            assert!(entries[0].1.contains("Day "));
        }
        other => panic!("expected the dossier list, got {other:?}"),
    }

    client.handle(Intent::Dossier);
    assert!(
        matches!(client.screen, Screen::Run),
        "the key closes it too"
    );
}

#[test]
fn a_first_time_hint_fires_once_and_never_again() {
    let mut client = run_on_seed("11");
    let mut seen: Vec<String> = Vec::new();
    // Waiting is enough: the hints are about state, not about what was done.
    for _ in 0..40 {
        client.handle(Intent::Wait);
        if let Some(hint) = client.hint.clone() {
            assert!(
                !seen.contains(&hint),
                "a first-time hint fired twice: {hint}"
            );
            seen.push(hint);
        }
        if client
            .run
            .as_ref()
            .is_none_or(|run| run.outcome().is_some())
        {
            break;
        }
    }
    assert!(!seen.is_empty(), "something should have been worth saying");
    for hint in &seen {
        assert!(hint.starts_with('['), "unbracketed hint: {hint}");
    }
}

#[test]
fn the_case_report_marks_the_certified_routes_against_what_was_done() {
    let mut client = run_on_seed("11");
    // The report is the same view whenever it is reached, so it can be read
    // without playing a run to its end.
    client.screen = Screen::CaseReport;

    match client.view().screen {
        rh_client_core::view::ScreenView::CaseReport(report) => {
            assert!(!report.tier.is_empty(), "the report says how far it got");
            assert!(
                !report.preparations.is_empty(),
                "a defeat should say what was short, not only that it went badly"
            );
            assert!(!report.routes.is_empty(), "the routes were certified");
            // The report used to build these three straight out of StringIds,
            // so it told players the villain was "villains.werewolf.name".
            let strings = client.catalogue.strings.clone();
            for field in [&report.villain, &report.origin, &report.scheme] {
                assert!(
                    !strings.ids().any(|id| field.contains(id)),
                    "the report shows a raw string id: {field:?}"
                );
                assert!(
                    !field.contains("!missing"),
                    "the report failed to resolve an id: {field:?}"
                );
            }
            for route in &report.routes {
                // Every step line carries a mark saying whether it was done.
                for line in route.lines().skip(1) {
                    // The whole step line composes through the table now, so
                    // its own placeholder brackets wrap the mark's: a line
                    // reads "[[not done] t0: [Resolve: ...]]".
                    assert!(
                        line.starts_with("[[done]")
                            || line.starts_with("[[not done]")
                            || line.starts_with("[[--]"),
                        "unmarked route step: {line}"
                    );
                }
            }
        }
        other => panic!("expected the case report, got {other:?}"),
    }
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

#[test]
fn splash_text_comes_from_the_string_table() {
    // The tracer bullet: ui.toml holds only ids now, so if the splash still
    // renders prose it can only have come through the string table. The
    // brackets are the marker that this copy is agent-written placeholder.
    let client = session();
    let view = rh_client_core::view::build(&client);
    let rh_client_core::view::ScreenView::Splash {
        title,
        intro,
        bindings,
        ..
    } = view.screen
    else {
        panic!("the session opens on the splash");
    };

    assert_eq!(title, "[ROGUE HUNTER]");
    assert!(!intro.is_empty(), "the splash has intro prose");
    for paragraph in &intro {
        assert!(
            paragraph.starts_with('[') && paragraph.ends_with(']'),
            "unbracketed splash paragraph: {paragraph:?}"
        );
    }
    for (keys, action) in &bindings {
        for text in [keys, action] {
            assert!(
                !text.contains("!missing"),
                "unresolved string id in the bindings table: {text:?}"
            );
            assert!(
                text.starts_with('[') && text.ends_with(']'),
                "unbracketed binding text: {text:?}"
            );
        }
    }
}

#[test]
fn an_opened_grave_reports_what_it_held_on_inspect() {
    // The log announces a grave's contents when it is opened, but that
    // scrolls away. Inspect has to be able to answer the question again for
    // a player re-walking a graveyard -- which is what grave_contents_name
    // was written for and never wired to.
    use rh_core::command::Command;
    use rh_core::world::FeatureKind;

    let mut client = run_on_seed("42");
    let run = client.run.as_mut().expect("a run is under way");
    let map = run.sim.state.current_map;
    let grave = run
        .sim
        .world
        .map(map)
        .features
        .iter()
        .find(|f| matches!(f.kind, FeatureKind::Grave { .. }))
        .map(|f| (f.id, f.at))
        .expect("the settlement has graves");

    // Unopened, inspect gives the name and nothing about the contents.
    let before = client.inspect(grave.1).expect("the grave is inspectable");
    assert!(
        !before.contains("(opened)"),
        "an unopened grave must not read as opened: {before:?}"
    );

    // Stand on it, pay the Physical point, and dig.
    let run = client.run.as_mut().expect("a run is under way");
    run.sim.state.hunter.pos = grave.1;
    run.sim.state.hunter.physical = run.sim.catalogue.hunter.physical_cap.max(1);
    run.apply(Command::OpenGrave(grave.0))
        .expect("standing on a grave with a Physical point, it opens");

    let after = client.inspect(grave.1).expect("the grave is inspectable");
    assert!(
        after.contains("(opened)"),
        "an opened grave must say so: {after:?}"
    );
    let contents = [
        "an empty grave",
        "an honest burial",
        "the villain's resting place",
    ];
    assert!(
        contents.iter().any(|c| after.contains(c)),
        "an opened grave must name what it held, got {after:?}"
    );
    assert!(
        !after.contains("!missing"),
        "the inspect line must resolve every id: {after:?}"
    );
}

#[test]
fn keys_mean_what_the_mode_says() {
    use rh_client_core::{InputMode, Key};

    // The splash is a menu: arrows navigate, and j/k follow them.
    let mut client = session();
    assert_eq!(client.input_mode(), InputMode::ListNav);
    assert_eq!(client.intent_for_key(Key::Down), Some(Intent::Down));
    assert_eq!(client.intent_for_key(Key::Char('j')), Some(Intent::Down));

    // Seed entry takes characters, not bindings.
    client.handle(Intent::Down);
    client.handle(Intent::Confirm);
    assert_eq!(client.input_mode(), InputMode::TextEntry);
    assert_eq!(
        client.intent_for_key(Key::Char('7')),
        Some(Intent::Char('7'))
    );

    // The run screen is tactical: the same keys move the hunter.
    let mut client = run_on_seed("11");
    assert_eq!(client.input_mode(), InputMode::Tactical);
    assert_eq!(
        client.intent_for_key(Key::Down),
        Some(Intent::Move(Direction::South))
    );
    assert_eq!(
        client.intent_for_key(Key::Char('7')),
        Some(Intent::Move(Direction::NorthWest))
    );
    assert_eq!(client.intent_for_key(Key::Tab), Some(Intent::NextThreat));

    // A modal flips the run screen back to list navigation.
    client.handle(Intent::Interact);
    if client.modal.is_some() {
        assert_eq!(client.input_mode(), InputMode::ListNav);
        assert_eq!(client.intent_for_key(Key::Down), Some(Intent::Down));
    }
}

#[test]
fn the_action_panel_prints_the_key_the_translator_honours() {
    let client = run_on_seed("11");
    for entry in client.available_actions() {
        let mut chars = entry.key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            // "Tab" and the walk row's return glyph are not char bindings.
            continue;
        };
        if c == '\u{21b5}' {
            continue;
        }
        assert_eq!(
            client.intent_for_key(rh_client_core::Key::Char(c)),
            Some(entry.intent.clone()),
            "panel key {c:?} must fire the intent its row promises"
        );
    }
}

#[test]
fn the_save_follows_the_run_and_only_the_run() {
    use rh_client_core::SaveAction;

    // No run, on the splash: whatever is stored is stale and goes.
    let client = session();
    assert_eq!(client.save_action(), SaveAction::Clear);

    // A live run is written wherever the player is looking.
    let mut client = run_on_seed("11");
    let code = client.share_code().expect("live run has a code");
    assert_eq!(client.save_action(), SaveAction::Write(code.clone()));
    client.handle(Intent::Grimoire);
    assert_eq!(client.save_action(), SaveAction::Write(code));
}
