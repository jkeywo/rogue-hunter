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
    assert_eq!(catalogue.hunter.stamina_cap, 4);

    // Both villain archetypes with their concealment styles.
    assert_eq!(
        catalogue.villains["werewolf"].concealment,
        Concealment::NpcHost
    );
    assert_eq!(
        catalogue.villains["revenant"].concealment,
        Concealment::DormantGrave
    );

    // The three ordinary enemy families.
    for family in ["wolf", "bandit", "restless-dead"] {
        assert!(
            catalogue.enemies.contains_key(family),
            "missing enemy family {family}"
        );
    }

    // Eight villain combinations need two origins and two schemes.
    assert_eq!(catalogue.origins.len(), 2);
    assert_eq!(catalogue.schemes.len(), 2);

    // The healing draught restores 4 health per the spec.
    match &catalogue.items["wound-draught"].kind {
        ItemKind::Draught { heal } => assert_eq!(*heal, 4),
        other => panic!("wound-draught has wrong kind: {other:?}"),
    }

    // Three maps forming the travel triangle.
    assert_eq!(catalogue.maps.len(), 3);
}

#[test]
fn catalogue_rejects_missing_files() {
    let error = rh_content::Catalogue::from_sources(&[("balance.toml", "")]);
    assert!(error.is_err());
}
