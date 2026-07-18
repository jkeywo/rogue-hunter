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
