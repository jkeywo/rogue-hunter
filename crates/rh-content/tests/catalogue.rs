//! The embedded content catalogue must always load and validate cleanly.
//! This test is the CI schema gate for hand-edited content files.

use rh_content::{Concealment, ItemKind};

#[test]
fn embedded_catalogue_loads_and_validates() {
    let catalogue = rh_content::load_embedded().expect("embedded content must validate");

    // Spot-check the fixed MVP profile the spec pins down.
    assert_eq!(catalogue.hunter.health, 12);
    assert_eq!(catalogue.hunter.lore_cap, 2);
    assert_eq!(catalogue.hunter.social_cap, 2);
    assert_eq!(catalogue.hunter.mystic_cap, 0);
    assert_eq!(catalogue.hunter.physical_cap, 2);
    assert_eq!(catalogue.hunter.stamina_cap, 6);

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
    // Every line in the table was written by an agent, and brackets are how we
    // say so. As real copy lands, delete the brackets and narrow this test --
    // it is a gate on unreviewed prose, not on the format.
    let catalogue = rh_content::load_embedded().expect("embedded content");
    for (id, row) in catalogue.strings.rows() {
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
    let orphans: Vec<&str> = catalogue
        .strings
        .ids()
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
