//! The embedded content catalogue must always load and validate cleanly.
//! This test is the CI schema gate for hand-edited content files.

use rh_content::{Concealment, ItemKind};

#[test]
fn embedded_catalogue_loads_and_validates() {
    let catalogue = rh_content::load_embedded().expect("embedded content must validate");

    // Spot-check the fixed MVP profile the spec pins down. Named rather than
    // taken from the default, which is the first hunter by id and is no longer
    // the Huntress now that the Advocate sorts ahead of her.
    let huntress = &catalogue.hunters["huntress"];
    assert_eq!(huntress.health, 12);
    assert_eq!(huntress.lore_cap, 2);
    assert_eq!(huntress.social_cap, 2);
    assert_eq!(huntress.mystic_cap, 0);
    assert_eq!(huntress.physical_cap, 2);
    assert_eq!(huntress.stamina_cap, 6);

    // Three villain archetypes with their concealment styles. The Witch
    // shares the Werewolf's NPC host but fights behind a ward.
    assert_eq!(
        catalogue.villains["werewolf"].concealment,
        Concealment::NpcHost
    );
    assert_eq!(
        catalogue.villains["revenant"].concealment,
        Concealment::DormantGrave
    );
    assert_eq!(
        catalogue.villains["witch"].concealment,
        Concealment::NpcHost
    );
    assert!(
        catalogue.villains["witch"].ward.is_some(),
        "the Witch fights through a hex-ward"
    );

    // The ordinary enemy families, including the Calling's thralls.
    for family in ["wolf", "bandit", "restless-dead", "thrall"] {
        assert!(
            catalogue.enemies.contains_key(family),
            "missing enemy family {family}"
        );
    }

    // Twenty-seven case compositions: three values on each of three axes.
    assert_eq!(catalogue.villains.len(), 3);
    assert_eq!(catalogue.origins.len(), 3);
    assert_eq!(catalogue.schemes.len(), 3);

    // Each origin demands its own counter reagent, which is what makes
    // reading the origin load-bearing rather than flavour.
    let mut reagents: Vec<&str> = catalogue
        .origins
        .values()
        .map(|origin| origin.counter_reagent.as_str())
        .collect();
    reagents.sort_unstable();
    reagents.dedup();
    assert_eq!(reagents.len(), 3, "origins must demand distinct reagents");

    // Every scheme offers exactly one pre-emption to blunt its escalation.
    for (id, scheme) in &catalogue.schemes {
        assert!(
            scheme.preempt.cost > 0,
            "scheme {id} pre-emption must cost a point"
        );
    }

    // The healing draught restores 4 health per the spec.
    match &catalogue.items["wound-draught"].kind {
        ItemKind::Draught { heal } => assert_eq!(*heal, 4),
        other => panic!("wound-draught has wrong kind: {other:?}"),
    }

    // Three roles form the travel triangle, and each must offer a choice of
    // templates: a run picks one per role, so a role with a single template
    // would always look the same.
    for role in rh_content::MapRole::ORDER {
        let templates = catalogue.templates_for(role);
        assert!(
            templates.len() >= 2,
            "role '{}' has only {:?}",
            role.label(),
            templates
        );
    }
}

#[test]
fn catalogue_rejects_missing_files() {
    let error = rh_content::Catalogue::from_sources(&[("balance.toml", "")]);
    assert!(error.is_err());
}

#[test]
fn openings_cover_every_bankable_node_kind() {
    let catalogue = rh_content::load_embedded().expect("embedded content");
    use rh_content::{OpeningAnchor, OpeningGrant};

    // Generation may bank any of these, so all must be narratable.
    for anchor in [OpeningAnchor::Tile, OpeningAnchor::Npc] {
        for grant in [
            OpeningGrant::Items,
            OpeningGrant::Lead,
            OpeningGrant::Identity,
        ] {
            assert!(
                catalogue.openings.iter().any(|o| o.matches(anchor, grant)),
                "nothing narrates a {anchor:?}-anchored {grant:?} node banked before play"
            );
        }
    }
    // Most runs bank nothing, so they need more than one way to begin.
    assert!(catalogue.openings.iter().filter(|o| o.is_generic()).count() >= 2);
}

#[test]
fn a_half_keyed_opening_is_rejected() {
    // An opening keyed on anchor but not grant would match nodes its prose
    // does not fit, so validation must refuse it.
    let mut sources: Vec<(&str, String)> = rh_content::embedded_sources()
        .iter()
        .map(|(name, text)| (*name, (*text).to_owned()))
        .collect();
    for (name, text) in sources.iter_mut() {
        if *name == "openings.toml" {
            text.push_str(
                "\n[[openings]]\nid = \"half-keyed\"\nanchor = \"npc\"\nbody = [\"A line.\"]\n",
            );
        }
    }
    let borrowed: Vec<(&str, &str)> = sources
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    let error = rh_content::Catalogue::from_sources(&borrowed);
    assert!(error.is_err(), "a half-keyed opening must not validate");
}

#[test]
fn the_string_table_is_outside_the_content_fingerprint() {
    // The whole point of holding strings apart from SOURCES: rewriting a line
    // or translating it must not invalidate a share code in the wild.
    assert!(
        !rh_content::embedded_sources()
            .iter()
            .any(|(name, _)| name.contains("strings")),
        "strings.csv must not be a fingerprinted source"
    );

    let before = rh_content::content_fingerprint();
    let mutated = rh_content::embedded_strings().replace("ROGUE HUNTER", "A QUITE DIFFERENT NAME");
    assert_ne!(
        mutated,
        rh_content::embedded_strings(),
        "the perturbation must actually change the table"
    );
    let catalogue =
        rh_content::Catalogue::from_sources_with_strings(rh_content::embedded_sources(), &mutated)
            .expect("a catalogue with rewritten copy still loads");
    assert_eq!(
        catalogue
            .strings
            .try_get("ui.splash.title")
            .expect("the title resolves"),
        "[A QUITE DIFFERENT NAME]"
    );
    assert_eq!(
        before,
        rh_content::content_fingerprint(),
        "rewriting copy must leave the content fingerprint alone"
    );
}

#[test]
fn every_string_is_bracketed_placeholder_copy() {
    // Every line of prose in the table was written by an agent, and brackets
    // are how we say so. As real copy lands, delete the brackets and narrow
    // this test -- it is a gate on unreviewed prose, not on the format.
    //
    // Terms are exempt, per `is_term`: naming a thing is not prose, and a
    // marked term nested its brackets inside whatever substituted it.
    let catalogue = rh_content::load_embedded().expect("embedded content");
    for (id, row) in catalogue.strings.rows() {
        if rh_content::is_term(&row.english) {
            continue;
        }
        assert!(
            row.english.starts_with('[') && row.english.ends_with(']'),
            "'{id}' is not marked as placeholder copy: {:?}",
            row.english
        );
    }
}

#[test]
fn every_string_row_carries_a_context_note() {
    let catalogue = rh_content::load_embedded().expect("embedded content");
    for (id, row) in catalogue.strings.rows() {
        assert!(!row.context.trim().is_empty(), "'{id}' has no context note");
    }
}

#[test]
fn string_ids_are_unique_and_sorted() {
    // Sorted so diffs stay reviewable and the file stays bisectable.
    let source = rh_content::embedded_strings();
    // Reading ids off raw lines is only sound while no cell spans lines. The
    // parser supports embedded newlines; the table deliberately does not use
    // them, and multi-paragraph prose is indexed ids instead.
    let catalogue = rh_content::load_embedded().expect("embedded content");
    assert_eq!(
        catalogue.strings.len(),
        source
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
            - 1,
        "no string-table cell may span lines"
    );
    let ids: Vec<&str> = source
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split(',').next().unwrap_or_default())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "strings.csv rows must be sorted by id");
    let mut deduped = sorted.clone();
    deduped.dedup();
    assert_eq!(deduped.len(), ids.len(), "string ids must be unique");
}

#[test]
fn an_unresolvable_string_id_is_refused() {
    let mut sources: Vec<(&str, String)> = rh_content::embedded_sources()
        .iter()
        .map(|(name, text)| (*name, (*text).to_owned()))
        .collect();
    for (name, text) in sources.iter_mut() {
        if *name == "ui.toml" {
            *text = text.replace("ui.splash.title", "ui.splash.title.typo");
        }
    }
    let borrowed: Vec<(&str, &str)> = sources
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    assert!(
        rh_content::Catalogue::from_sources(&borrowed).is_err(),
        "content pointing at a missing string id must not validate"
    );
}

#[test]
fn a_church_clue_must_declare_its_slot() {
    // The generator once picked the church slot by sniffing the clue's display
    // name for "candle", which made world layout depend on prose that is now
    // localised. Placement is authored, and validation is what keeps it so.
    let mut sources: Vec<(&str, String)> = rh_content::embedded_sources()
        .iter()
        .map(|(name, text)| (*name, (*text).to_owned()))
        .collect();
    for (name, text) in sources.iter_mut() {
        if *name == "clues.toml" {
            // Content files are CRLF; match the key alone so the assertion
            // does not quietly depend on line endings.
            *text = text.replace("church_slot = \"records\"", "");
        }
    }
    let borrowed: Vec<(&str, &str)> = sources
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    assert!(
        rh_content::Catalogue::from_sources(&borrowed).is_err(),
        "a church clue without a church_slot must not validate"
    );
}

#[test]
fn every_axis_is_worth_drawing_from_and_mostly_texture() {
    let catalogue = rh_content::load_embedded().expect("embedded content");
    use rh_content::ConditionAxis;

    for axis in [
        ConditionAxis::Season,
        ConditionAxis::Reception,
        ConditionAxis::Hour,
        ConditionAxis::Provenance,
    ] {
        let on_axis: Vec<_> = catalogue
            .conditions
            .iter()
            .filter(|c| c.axis == axis)
            .collect();
        assert!(
            on_axis.len() >= 5,
            "axis {axis:?} has only {}",
            on_axis.len()
        );
        // Any axis may be the one that bites this run, or the one that helps,
        // so each needs exactly one of each to draw from — and neutrals for
        // the two axes that come up texture.
        assert_eq!(
            on_axis.iter().filter(|c| c.is_bane()).count(),
            1,
            "axis {axis:?}"
        );
        assert_eq!(
            on_axis.iter().filter(|c| c.is_boon()).count(),
            1,
            "axis {axis:?}"
        );
        assert!(
            on_axis.iter().filter(|c| c.is_cosmetic()).count() >= 3,
            "axis {axis:?}"
        );
    }
    assert!(catalogue.openings.iter().filter(|o| o.is_generic()).count() >= 6);
}

#[test]
fn a_condition_may_not_name_a_clue_or_an_informant() {
    // Conditions are drawn whether or not a node was banked, so those
    // placeholders would resolve to nothing.
    let mut sources: Vec<(&str, String)> = rh_content::embedded_sources()
        .iter()
        .map(|(name, text)| (*name, (*text).to_owned()))
        .collect();
    for (name, text) in sources.iter_mut() {
        if *name == "openings.toml" {
            text.push_str(
                "\n[[conditions]]\nid = \"names-a-person\"\naxis = \"hour\"\nbody = [\"{npc} was waiting.\"]\n",
            );
        }
    }
    let borrowed: Vec<(&str, &str)> = sources
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    assert!(rh_content::Catalogue::from_sources(&borrowed).is_err());
}

#[test]
fn every_string_in_the_table_is_referenced_by_content() {
    // Both directions matter. `check_strings` proves every id the content
    // names resolves; this proves the table has no rows nothing reaches --
    // dead copy a translator would otherwise be asked to work on.
    let catalogue = rh_content::load_embedded().expect("embedded content");
    let referenced: std::collections::BTreeSet<&str> =
        rh_content::referenced_string_ids(&catalogue)
            .into_iter()
            .map(|(_, id)| id.as_str())
            .collect();
    // `ui.*` ids are named by the clients rather than by content, so the
    // catalogue walker cannot see them; `ui_string_ids_match_the_code` is
    // what holds that namespace to the same standard.
    let orphans: Vec<&str> = catalogue
        .strings
        .ids()
        .filter(|id| !id.starts_with("ui.") && !id.starts_with("log.") && !id.starts_with("gen."))
        .filter(|id| !referenced.contains(id))
        .collect();
    assert!(orphans.is_empty(), "unreferenced string rows: {orphans:?}");
}

#[test]
fn a_typo_in_any_content_string_id_is_refused() {
    // The resolve check must cover the whole catalogue, not just the file it
    // was first written for.
    let mut sources: Vec<(&str, String)> = rh_content::embedded_sources()
        .iter()
        .map(|(name, text)| (*name, (*text).to_owned()))
        .collect();
    for (name, text) in sources.iter_mut() {
        if *name == "clues.toml" {
            *text = text.replace("clues.second-face.reveal", "clues.second-face.rveeal");
        }
    }
    let borrowed: Vec<(&str, &str)> = sources
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    assert!(
        rh_content::Catalogue::from_sources(&borrowed).is_err(),
        "a mistyped clue string id must not validate"
    );
}

/// Every code-side string id (`ui.*`, `log.*`, `gen.*`) named in the crates.
///
/// The catalogue cannot reach these -- code names them as literals -- so they
/// are checked against the source text instead. Matching the literal rather
/// than the call syntax keeps this honest as new helpers appear.
fn code_side_ids() -> std::collections::BTreeSet<String> {
    let pattern = ["ui.", "log.", "gen."];
    let mut found = std::collections::BTreeSet::new();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir");
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable crate dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            // This test file names ids in prose and in filters; counting them
            // would let it satisfy itself.
            if path.ends_with("tests/catalogue.rs") || path.ends_with("catalogue.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable source");
            for piece in text.split('"').skip(1).step_by(2) {
                // Real ids have at least three segments, which rules out the
                // "ui.toml" filename and the two-segment fixtures in unit
                // tests without needing to know where either lives.
                if pattern.iter().any(|p| piece.starts_with(p))
                    && piece.split('.').count() >= 3
                    // A trailing dot means a namespace prefix, not an id --
                    // tests match on those to select a whole namespace.
                    && !piece.ends_with('.')
                    && piece.chars().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-'
                    })
                {
                    found.insert(piece.to_owned());
                }
            }
        }
    }
    found
}

#[test]
fn code_side_string_ids_match_the_code() {
    let catalogue = rh_content::load_embedded().expect("embedded content");
    let in_code = code_side_ids();
    assert!(
        in_code.len() > 50,
        "the scanner found only {} ids, so it is not testing anything",
        in_code.len()
    );

    let missing: Vec<&String> = in_code
        .iter()
        .filter(|id| catalogue.strings.try_get(id).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "code names string ids that are not in the table: {missing:?}"
    );

    let orphans: Vec<&str> = catalogue
        .strings
        .ids()
        .filter(|id| id.starts_with("ui.") || id.starts_with("log.") || id.starts_with("gen."))
        // ui.toml holds these, so content references them, not code.
        .filter(|id| !id.starts_with("ui.keys.") && !id.starts_with("ui.splash.intro"))
        .filter(|id| *id != "ui.splash.title")
        .filter(|id| !in_code.contains(*id))
        .collect();
    assert!(
        orphans.is_empty(),
        "code-side rows no code reaches: {orphans:?}"
    );
}

#[test]
fn a_slot_walled_off_from_the_map_is_rejected() {
    // Cover and blocked-lane geometry is exactly what a variation pack will
    // rewrite, and a slot sealed behind it would break a certified route in
    // silence: the planner reasons about maps and gates, never about whether
    // the tiles actually join up.
    let mut sources: Vec<(&str, String)> = rh_content::embedded_sources()
        .iter()
        .map(|(name, text)| (*name, (*text).to_owned()))
        .collect();
    for (name, text) in sources.iter_mut() {
        if *name == "maps/settlement.toml" {
            // Wall in the church altar's corner.
            *text = text.replace("\"#.A.....+", "\"#.A#####+").replace(
                "\"#.......#,,,,,,,,,,,,,,,#......#",
                "\"#.###...#,,,,,,,,,,,,,,,#......#",
            );
        }
    }
    let borrowed: Vec<(&str, &str)> = sources
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    let error = rh_content::Catalogue::from_sources(&borrowed);
    let message = format!(
        "{:?}",
        error.expect_err("a sealed-off slot must not validate")
    );
    assert!(
        message.contains("cannot be walked to"),
        "expected a reachability complaint, got: {message}"
    );
}

/// The content fingerprint must describe the content, not the machine that
/// compiled it.
///
/// This number rides inside every `ReplayRecord` and is checked on load, so a
/// build that computes it differently refuses share codes from a build that
/// does not. That is exactly what used to happen: the content files are stored
/// with LF and checked out with CRLF on Windows, `include_str!` embeds
/// whatever is on disk, and the native Windows build and the wasm build --
/// which is compiled on Linux -- quietly disagreed about which content they
/// were running. Nothing logged it; codes simply came back ContentMismatch.
///
/// Re-hashing the sources with the endings flipped is the closest a test on
/// one platform can get to asking what the other would compute.
#[test]
fn the_fingerprint_does_not_depend_on_line_endings() {
    fn fingerprint_of(sources: &[(String, String)]) -> u16 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for (name, source) in sources {
            for byte in name.bytes().chain(source.bytes()).filter(|b| *b != b'\r') {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        (hash ^ (hash >> 16) ^ (hash >> 32) ^ (hash >> 48)) as u16
    }

    let as_lf: Vec<(String, String)> = rh_content::embedded_sources()
        .iter()
        .map(|(name, source)| ((*name).to_owned(), source.replace("\r\n", "\n")))
        .collect();
    let as_crlf: Vec<(String, String)> = as_lf
        .iter()
        .map(|(name, source)| (name.clone(), source.replace('\n', "\r\n")))
        .collect();

    assert_eq!(
        fingerprint_of(&as_lf),
        fingerprint_of(&as_crlf),
        "the same content hashes differently depending on its line endings, so \
         a share code recorded on one platform will be refused on another"
    );
    assert_eq!(
        rh_content::content_fingerprint(),
        fingerprint_of(&as_lf),
        "this checkout's fingerprint disagrees with the normalised one"
    );
}
