//! Combat-viability heuristic shared by the planner and the inspector.
//!
//! A deterministic, integer-only estimate (permille) of the hunter's chance
//! in a final fight, given a prepared loadout and the villain's threat tier.
//! The generator certifies routes only when this clears the authored
//! `viability_threshold_permille`; bad-roll losses remain possible in play.
//!
//! The model is deliberately coarse: expected damage-per-turn races with
//! flat bonuses for snares (denial), draughts (effective health at an action
//! cost), and a Killing Blow burst. Its purpose is comparative gating of
//! generated worlds, not exact win probability.

use rh_content::{Catalogue, ItemKind, ManoeuvreEffect};

/// What the hunter brings to the hunt, as tracked by the planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HuntLoadout {
    pub hunter_hp: u16,
    pub draughts: u16,
    pub silver_bullets: u16,
    pub binding_charms: u16,
    /// Physical points available for snares / Killing Blow.
    pub physical: u8,
    /// The fight happens on consecrated ground (revenant church route).
    pub on_consecrated_ground: bool,
    /// The hunt opens on the dormant villain in its grave (coup de grace).
    pub dormant_opening: bool,
}

/// Estimated final-fight viability in permille (750 = 75%).
pub fn hunt_viability(
    catalogue: &Catalogue,
    villain_id: &str,
    tier: u8,
    loadout: &HuntLoadout,
) -> u16 {
    let Some(villain) = catalogue.villains.get(villain_id) else {
        return 0;
    };
    let combat = &catalogue.balance.combat;

    // --- Hunter offence: expected damage per turn, in millis. -------------
    let blade: u16 = catalogue
        .hunter
        .starting_items
        .iter()
        .filter_map(
            |item| match catalogue.items.get(item).map(|def| &def.kind) {
                Some(ItemKind::MeleeWeapon { damage }) => Some(*damage),
                _ => None,
            },
        )
        .max()
        .unwrap_or(1);
    let power_numerator: u32 = catalogue
        .hunter
        .manoeuvres
        .iter()
        .find_map(|m| match m.effect {
            ManoeuvreEffect::MeleeDamageMultiplier { numerator } => Some(u32::from(numerator)),
            _ => None,
        })
        .unwrap_or(2);
    // Plain swings every turn: priming Power Attack costs an action, which
    // makes the primed cycle a damage LOSS at MVP blade values. The
    // multiplier only pays off against a sleeping target (the coup opener).
    let melee_dpt = u32::from(blade) * 1000 * u32::from(combat.melee_hit_percent) / 100;

    // --- Villain durability and offence. -----------------------------------
    let villain_hp =
        u32::from(villain.health + villain.tier_bonus_health * u16::from(tier.min(2))) * 1000;
    let tier_damage: u16 = villain
        .tier_behaviours
        .iter()
        .take(usize::from(tier))
        .map(|behaviour| match behaviour.effect {
            rh_content::TierEffect::BonusMeleeDamage { amount } => amount,
            _ => 0,
        })
        .sum();
    let mut incoming =
        u32::from(villain.melee_damage + tier_damage) * 10 * u32::from(villain.hit_percent);
    // Snares and trapped-attack penalties blunt the villain's offence.
    if loadout.physical > 0 {
        incoming = incoming * 9 / 10;
    }
    let incoming = incoming.max(100);

    // --- Turns to kill. ------------------------------------------------------
    let turns_to_kill = match &villain.cadence {
        None => {
            // Werewolf-style: always woundable, defended by regeneration.
            let mut effective_hp = villain_hp;
            let mut setup_turns: i32 = 0;
            let mut regen_millis = villain
                .regeneration
                .as_ref()
                .map(|regen| u32::from(regen.per_turn) * 1000)
                .unwrap_or(0);
            if loadout.silver_bullets > 0 {
                if let Some(ItemKind::WeaknessAmmunition {
                    damage,
                    stops_regeneration,
                }) = catalogue
                    .items
                    .get(&villain.weakness_item)
                    .map(|def| &def.kind)
                {
                    effective_hp = effective_hp.saturating_sub(u32::from(*damage) * 1000);
                    if *stops_regeneration {
                        regen_millis = 0;
                    }
                    setup_turns += 2; // aim + certain shot
                }
            }
            let net = melee_dpt.saturating_sub(regen_millis).max(100);
            let mut turns = (effective_hp / net) as i32 + setup_turns;
            if loadout.physical > 0 {
                turns -= 2; // Killing Blow burst once it is wounded
            }
            turns.max(1)
        }
        Some(cadence) => {
            // Revenant-style: damage lands only in vulnerability windows,
            // where it bites twice as deep.
            let vulnerable_dpt = melee_dpt * 2;
            let mut remaining = villain_hp;
            let mut turns: i32 = 0;
            if loadout.dormant_opening && loadout.physical > 0 {
                // Coup de grace on the dormant thing in its grave.
                let opener =
                    u32::from(blade) * power_numerator * 1000 / 2 * 2 /* killing blow */ * 2 /* coup */;
                remaining = remaining.saturating_sub(opener);
                turns += 1;
            }
            if loadout.on_consecrated_ground {
                let dpt = vulnerable_dpt + u32::from(cadence.consecrated_damage_per_turn) * 1000;
                turns += (remaining / dpt.max(100)) as i32 + 1;
            } else {
                let charm_turns =
                    u32::from(loadout.binding_charms) * u32::from(cadence.bound_vulnerable_turns);
                let charm_damage = charm_turns * vulnerable_dpt;
                if charm_damage >= remaining {
                    turns += (remaining / vulnerable_dpt.max(100)) as i32
                        + i32::from(loadout.binding_charms > 0); // the turn spent placing it
                } else {
                    remaining -= charm_damage;
                    turns += charm_turns as i32 + i32::from(loadout.binding_charms > 0);
                    // The rest must land in natural windows: one turn in
                    // `period` at double damage.
                    let guarded_dpt = (vulnerable_dpt / u32::from(cadence.period)).max(100);
                    turns += (remaining / guarded_dpt) as i32;
                }
            }
            turns.max(1)
        }
    };

    // --- Turns the hunter survives. -----------------------------------------
    let draught_heal: u16 = match catalogue.items.get("wound-draught").map(|def| &def.kind) {
        Some(ItemKind::Draught { heal }) => *heal,
        _ => 4,
    };
    let effective_hp = u32::from(loadout.hunter_hp + draught_heal * loadout.draughts) * 1000;
    let mut survive = (effective_hp / incoming) as i32;
    survive += i32::from(loadout.physical.min(2)) * 3; // snare denial
    survive -= i32::from(loadout.draughts) as i32; // drinking costs actions
    if loadout.on_consecrated_ground {
        survive += 1; // the ward burns the revenant even as it approaches
    }

    let viability = 500 + (survive - turns_to_kill) * 75;
    viability.clamp(0, 1000) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalogue() -> Catalogue {
        rh_content::load_embedded().expect("embedded content")
    }

    #[test]
    fn unprepared_werewolf_hunt_is_not_viable() {
        let loadout = HuntLoadout {
            hunter_hp: 12,
            draughts: 0,
            silver_bullets: 0,
            binding_charms: 0,
            physical: 0,
            on_consecrated_ground: false,
            dormant_opening: false,
        };
        let viability = hunt_viability(&catalogue(), "werewolf", 0, &loadout);
        assert!(viability < 500, "unprepared hunt scored {viability}");
    }

    #[test]
    fn prepared_werewolf_hunt_clears_threshold() {
        let cat = catalogue();
        let loadout = HuntLoadout {
            hunter_hp: 12,
            draughts: 1,
            silver_bullets: 1,
            binding_charms: 0,
            physical: 2,
            on_consecrated_ground: false,
            dormant_opening: false,
        };
        let viability = hunt_viability(&cat, "werewolf", 1, &loadout);
        let threshold = cat.balance.generator.viability_threshold_permille;
        assert!(
            viability >= threshold,
            "prepared early hunt scored {viability}"
        );
    }

    #[test]
    fn consecrated_revenant_hunt_clears_threshold() {
        let cat = catalogue();
        let loadout = HuntLoadout {
            hunter_hp: 12,
            draughts: 2,
            silver_bullets: 0,
            binding_charms: 0,
            physical: 2,
            on_consecrated_ground: true,
            dormant_opening: false,
        };
        let viability = hunt_viability(&cat, "revenant", 2, &loadout);
        let threshold = cat.balance.generator.viability_threshold_permille;
        assert!(
            viability >= threshold,
            "consecrated final hunt scored {viability}"
        );
    }

    #[test]
    fn charm_revenant_early_hunt_clears_threshold() {
        let cat = catalogue();
        let loadout = HuntLoadout {
            hunter_hp: 12,
            draughts: 1,
            silver_bullets: 0,
            binding_charms: 1,
            physical: 2,
            on_consecrated_ground: false,
            dormant_opening: true,
        };
        let viability = hunt_viability(&cat, "revenant", 1, &loadout);
        let threshold = cat.balance.generator.viability_threshold_permille;
        assert!(
            viability >= threshold,
            "charm early hunt scored {viability}"
        );
    }

    #[test]
    fn higher_tier_never_improves_viability() {
        let cat = catalogue();
        let loadout = HuntLoadout {
            hunter_hp: 12,
            draughts: 1,
            silver_bullets: 1,
            binding_charms: 0,
            physical: 2,
            on_consecrated_ground: false,
            dormant_opening: false,
        };
        let tier0 = hunt_viability(&cat, "werewolf", 0, &loadout);
        let tier2 = hunt_viability(&cat, "werewolf", 2, &loadout);
        assert!(tier0 >= tier2, "tier 0 {tier0} vs tier 2 {tier2}");
    }
}
