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

use rh_content::{Catalogue, ItemKind, ManoeuvreEffect, SignatureEffect};

use crate::combat;

/// What the hunter brings to the hunt, as tracked by the planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HuntLoadout {
    pub hunter_hp: u16,
    pub draughts: u16,
    pub silver_bullets: u16,
    pub binding_charms: u16,
    /// Cold-iron blades: the counter that cuts through a hex-ward.
    pub counter_blades: u16,
    /// Physical points available for snares / Killing Blow.
    pub physical: u8,
    /// The fight happens on consecrated ground (revenant church route).
    pub on_consecrated_ground: bool,
    /// The hunt opens on the dormant villain in its grave (coup de grace).
    pub dormant_opening: bool,
}

impl HuntLoadout {
    /// What a kit is worth in the hunt, derived beside the model that prices
    /// it so a caller cannot invent a mapping of its own — that is how the
    /// estimate once credited a hunter with a finisher she did not have.
    /// `item_count` answers for whatever inventory the caller tracks; the
    /// openings (`dormant_opening`) stay the caller's choice to weigh.
    pub fn from_kit(
        catalogue: &Catalogue,
        item_count: impl Fn(&str) -> u16,
        physical: u8,
        on_consecrated_ground: bool,
    ) -> Self {
        Self {
            hunter_hp: catalogue.hunter.health,
            draughts: item_count("wound-draught"),
            silver_bullets: item_count("silver-bullet"),
            binding_charms: item_count("binding-charm"),
            counter_blades: item_count("cold-iron-pin"),
            physical,
            on_consecrated_ground,
            dormant_opening: false,
        }
    }
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
    // A hunter with no damage-multiplier manoeuvre swings at plain strength,
    // which in halves is `MULTIPLIER_HALVES` over itself — a stated 1x rather
    // than the bare 2 this once defaulted to, right only by coincidence that
    // the denominator happens to be 2.
    let power_numerator: u32 = catalogue
        .hunter
        .manoeuvres
        .iter()
        .find_map(|m| match m.effect {
            ManoeuvreEffect::MeleeDamageMultiplier { numerator } => Some(u32::from(numerator)),
            _ => None,
        })
        .unwrap_or(u32::from(combat::MULTIPLIER_HALVES));
    // Plain swings every turn: priming Power Attack costs an action, which
    // makes the primed cycle a damage LOSS at MVP blade values. The
    // multiplier only pays off against a sleeping target (the coup opener).
    let mut melee_dpt = u32::from(blade) * 1000 * u32::from(combat.melee_hit_percent) / 100;

    // What this hunter can actually do with a Physical point. The model used to
    // read "physical > 0" as "snare plus Killing Blow", which was the Huntress's
    // kit stated as though it were everyone's: it credited the Occultist with a
    // finisher she does not have and ignored the warded ground she does.
    let signature = |matcher: fn(&SignatureEffect) -> bool| {
        catalogue
            .hunter
            .signatures
            .iter()
            .find(|def| matcher(&def.effect))
            .map(|def| def.physical_cost)
    };
    let snare_cost = signature(|effect| matches!(effect, SignatureEffect::SetSnare));
    let killing_blow_cost = signature(|effect| matches!(effect, SignatureEffect::KillingBlow));
    let ward_cost = signature(|effect| matches!(effect, SignatureEffect::WardTheGround { .. }));
    let affords = |cost: Option<u8>| cost.is_some_and(|cost| loadout.physical >= cost);
    let has_snare = affords(snare_cost);
    let has_killing_blow = affords(killing_blow_cost);
    let has_ward = affords(ward_cost);

    // The Advocate's second: a villager who stands with her adds blows and
    // takes some in return. Priced in the model's own two currencies rather
    // than a new axis — damage a turn here, turns survived below — so a hunter
    // who fights through people is one the estimate can still vouch for. Read
    // the authored numbers off the signature; the cost gate is the same.
    let second = catalogue
        .hunter
        .signatures
        .iter()
        .find_map(|def| match &def.effect {
            SignatureEffect::SecondInTheFight {
                turns,
                damage_per_turn,
            } if loadout.physical >= def.physical_cost => Some((*turns, *damage_per_turn)),
            _ => None,
        });

    // Warded ground tears at the thing every time it comes across. Credited at
    // half rate: it bites on the approach and on repositioning, not every turn
    // of a toe-to-toe exchange.
    if has_ward {
        melee_dpt += u32::from(combat.ground_ward_damage) * 1000 / 2;
    }
    // The second's blows, credited at half rate like the ward: it stands for
    // only part of the fight, so its damage is a share of a full turn's, not
    // a whole one added for the duration.
    if let Some((_, damage_per_turn)) = second {
        melee_dpt +=
            u32::from(damage_per_turn) * 1000 * u32::from(combat.melee_hit_percent) / 100 / 2;
    }
    let melee_dpt = melee_dpt;

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
    // Snares and trapped-attack penalties blunt the villain's offence; so, less
    // reliably, does ground it does not want to stand on.
    if has_snare {
        incoming = incoming * 9 / 10;
    } else if has_ward {
        incoming = incoming * 19 / 20;
    }
    let incoming = incoming.max(100);

    // --- Turns to kill. ------------------------------------------------------
    let counter_blade_damage = match catalogue.items.get(&villain.weakness_item).map(|d| &d.kind) {
        Some(ItemKind::WeaknessBlade { damage }) => Some(u32::from(*damage)),
        _ => None,
    };
    let turns_to_kill = if let Some(ward) = &villain.ward {
        // Witch-style: a hex-ward soaks honest blows and is rewoven after it
        // breaks, so steel grinds through a cycle at a time. Cold iron ignores
        // the ward completely, which is the whole point of carrying it.
        match (loadout.counter_blades > 0, counter_blade_damage) {
            (true, Some(blade)) => {
                let dpt = (blade * 1000 * u32::from(combat.melee_hit_percent) / 100).max(100);
                ((villain_hp / dpt) as i32).max(1)
            }
            _ => {
                // One full cycle: `charges` blows soak down to a leak, then the
                // ward is down for `reweave_turns` of honest damage.
                let cycle_turns = u32::from(ward.charges) + u32::from(ward.reweave_turns);
                let cycle_damage = u32::from(ward.charges) * u32::from(ward.leak_damage) * 1000
                    + u32::from(ward.reweave_turns) * melee_dpt;
                let average = (cycle_damage / cycle_turns.max(1)).max(100);
                ((villain_hp / average) as i32).max(1)
            }
        }
    } else {
        match &villain.cadence {
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
                if has_killing_blow {
                    turns -= 2; // Killing Blow burst once it is wounded
                }
                turns.max(1)
            }
            Some(cadence) => {
                // Revenant-style: damage lands only in vulnerability windows,
                // where it bites twice as deep.
                let vulnerable_dpt = melee_dpt * u32::from(combat::VULNERABILITY_MULTIPLIER);
                let mut remaining = villain_hp;
                let mut turns: i32 = 0;
                if loadout.dormant_opening {
                    // Coup de grace on the dormant thing in its grave. The coup
                    // multiplier is for striking something asleep and is open
                    // to anyone; only the Killing Blow bonus needs the
                    // signature.
                    let finisher = if has_killing_blow {
                        u32::from(combat::KILLING_BLOW_MULTIPLIER)
                    } else {
                        1
                    };
                    let opener = u32::from(blade) * power_numerator * 1000
                        / u32::from(combat::MULTIPLIER_HALVES)
                        * finisher
                        * u32::from(combat::COUP_MULTIPLIER);
                    remaining = remaining.saturating_sub(opener);
                    turns += 1;
                }
                if loadout.on_consecrated_ground {
                    let dpt =
                        vulnerable_dpt + u32::from(cadence.consecrated_damage_per_turn) * 1000;
                    turns += (remaining / dpt.max(100)) as i32 + 1;
                } else {
                    let charm_turns = u32::from(loadout.binding_charms)
                        * u32::from(cadence.bound_vulnerable_turns);
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
        }
    };

    // --- Turns the hunter survives. -----------------------------------------
    let draught_heal: u16 = match catalogue.items.get("wound-draught").map(|def| &def.kind) {
        Some(ItemKind::Draught { heal }) => *heal,
        _ => 4,
    };
    let effective_hp = u32::from(loadout.hunter_hp + draught_heal * loadout.draughts) * 1000;
    let mut survive = (effective_hp / incoming) as i32;
    // Denial: turns the villain spends held, or crossing ground that hurts it,
    // rather than hitting the hunter. Both are real; neither is free to a
    // hunter who cannot perform them.
    if has_snare {
        survive += i32::from(loadout.physical.min(2)) * 3;
    } else if has_ward {
        // A snare is one tile, sprung once. Warded ground is an area that keeps
        // hurting whatever crosses it for as long as it lasts, so a single
        // Physical point spent on it buys more denial than one spent on a
        // snare — though still less than the Huntress's two points buy her.
        //
        // Five puts her preparation burden alongside the Huntress's rather than
        // strictly above it: at four she needed a wound draught for fights the
        // Huntress takes without one. It is worth being clear that this value
        // does not drive the rate at which her worlds fail to certify — that was
        // measured to be identical at four and five, and is a planner cost
        // documented under planner-cost-scales-with-mystic-pool.
        survive += 5;
    }
    // The second takes half of what comes at her while it stands, so across
    // its `turns` it buys back about half of them in survival. This is the
    // Advocate's whole survival case — she has neither the health to trade nor
    // a snare to deny — so unlike the snare and ward above it stacks rather
    // than being one branch of a choice.
    if let Some((turns, _)) = second {
        survive += i32::from(turns) / 2;
    }
    survive -= i32::from(loadout.draughts); // drinking costs actions
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
            counter_blades: 0,
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
            counter_blades: 0,
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
    fn the_advocates_second_is_what_carries_her() {
        // The Advocate has neither the health to trade nor a snare to deny, so
        // the estimate must be pricing her second — and only her second — when
        // it vouches for her. Score her loadout with the Physical point that
        // buys the second and without it; the gap is the second alone, since
        // nothing else she carries turns on that point.
        let cat = catalogue().with_hunter("advocate").expect("advocate");
        let with_point = HuntLoadout {
            hunter_hp: cat.hunter.health,
            draughts: 1,
            silver_bullets: 1,
            binding_charms: 0,
            counter_blades: 0,
            physical: 1,
            on_consecrated_ground: false,
            dormant_opening: false,
        };
        let without_point = HuntLoadout {
            physical: 0,
            ..with_point
        };
        let with_second = hunt_viability(&cat, "werewolf", 0, &with_point);
        let without = hunt_viability(&cat, "werewolf", 0, &without_point);

        assert!(
            with_second > without,
            "the second added nothing: {with_second} vs {without}"
        );
        let threshold = cat.balance.generator.viability_threshold_permille;
        assert!(
            with_second >= threshold,
            "the second should make the frail hunt viable, scored {with_second}"
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
            counter_blades: 0,
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
            counter_blades: 0,
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
            counter_blades: 0,
            physical: 2,
            on_consecrated_ground: false,
            dormant_opening: false,
        };
        let tier0 = hunt_viability(&cat, "werewolf", 0, &loadout);
        let tier2 = hunt_viability(&cat, "werewolf", 2, &loadout);
        assert!(tier0 >= tier2, "tier 0 {tier0} vs tier 2 {tier2}");
    }
}

#[cfg(test)]
mod hunter_comparison {
    use super::*;

    #[test]
    #[ignore = "diagnostic: prints viability per hunter; run with --ignored"]
    fn print_viability_per_hunter() {
        let base = rh_content::load_embedded().expect("content");
        for hunter in base.hunters.keys() {
            let cat = base.clone().with_hunter(hunter).expect("hunter");
            println!(
                "{hunter}: hp={} physical_cap={} signatures={:?}",
                cat.hunter.health,
                cat.hunter.physical_cap,
                cat.hunter
                    .signatures
                    .iter()
                    .map(|s| s.id.clone())
                    .collect::<Vec<_>>()
            );
            for villain in cat.villains.keys() {
                for (label, draughts, dormant, consecrated) in [
                    ("d0      ", 0u16, false, false),
                    ("d1      ", 1, false, false),
                    ("d1+grave", 1, true, false),
                    ("d1+chrch", 1, false, true),
                    ("d2      ", 2, false, false),
                ] {
                    let loadout = HuntLoadout {
                        hunter_hp: cat.hunter.health,
                        draughts,
                        silver_bullets: 1,
                        binding_charms: 1,
                        counter_blades: 1,
                        physical: cat.hunter.physical_cap,
                        on_consecrated_ground: consecrated,
                        dormant_opening: dormant,
                    };
                    let v = hunt_viability(&cat, villain, 1, &loadout);
                    println!("{hunter:10} vs {villain:10} {label} -> {v}");
                }
            }
        }
        println!(
            "threshold = {}",
            base.balance.generator.viability_threshold_permille
        );
    }
}
