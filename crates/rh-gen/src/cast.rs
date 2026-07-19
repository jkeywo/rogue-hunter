//! Villain combination and NPC cast selection.

use rh_content::{Catalogue, Concealment};
use rh_core::rng::SimRng;
use rh_core::world::Disposition;

/// The seed-picked villain combination.
#[derive(Debug, Clone)]
pub struct Combo {
    pub villain: String,
    pub origin: String,
    pub scheme: String,
}

/// One cast member, pre-materialisation.
#[derive(Debug, Clone)]
pub struct CastMember {
    pub archetype: String,
    pub name: String,
    pub disposition: Disposition,
    pub mystical: bool,
    pub trades: bool,
    pub secret_template: String,
    /// Whether this NPC secretly hosts the villain (werewolf runs).
    pub is_host: bool,
}

#[derive(Debug, Clone)]
pub struct Cast {
    pub members: Vec<CastMember>,
    /// Relationship triangle: (a index, b index, kind id).
    pub links: Vec<(usize, usize, String)>,
}

pub fn pick_combo(rng: &mut SimRng, catalogue: &Catalogue) -> Combo {
    let villains: Vec<&String> = catalogue.villains.keys().collect();
    let origins: Vec<&String> = catalogue.origins.keys().collect();
    let schemes: Vec<&String> = catalogue.schemes.keys().collect();
    Combo {
        villain: villains[rng.index(villains.len())].clone(),
        origin: origins[rng.index(origins.len())].clone(),
        scheme: schemes[rng.index(schemes.len())].clone(),
    }
}

/// Cast three NPCs: the mystical archetype plus two others, a full
/// relationship triangle, and (for NPC-host villains) the host.
pub fn pick_cast(rng: &mut SimRng, catalogue: &Catalogue, combo: &Combo) -> Result<Cast, String> {
    let mystical_id = catalogue
        .npcs
        .archetypes
        .iter()
        .find(|(_, def)| def.mystical)
        .map(|(id, _)| id.clone())
        .ok_or_else(|| "no mystical archetype in content".to_owned())?;

    let mut others: Vec<String> = catalogue
        .npcs
        .archetypes
        .keys()
        .filter(|id| **id != mystical_id)
        .cloned()
        .collect();
    // Draw two distinct non-mystical archetypes.
    let first = others.remove(rng.index(others.len()));
    let second = others.remove(rng.index(others.len()));
    let chosen = [mystical_id, first, second];

    // Dispositions: at most one hostile, and never the mystical NPC.
    let hostile_index = match rng.below(4) {
        0 => Some(1),
        1 => Some(2),
        _ => None,
    };

    let villain_def = &catalogue.villains[&combo.villain];
    let needs_host = villain_def.concealment == Concealment::NpcHost;
    let host_candidates: Vec<usize> = chosen
        .iter()
        .enumerate()
        .filter(|(_, id)| catalogue.npcs.archetypes[*id].can_host_villain)
        .map(|(index, _)| index)
        .collect();
    if needs_host && host_candidates.is_empty() {
        return Err("cast has no archetype that can host the villain".to_owned());
    }
    let host_index = if needs_host {
        Some(host_candidates[rng.index(host_candidates.len())])
    } else {
        None
    };

    let mut members = Vec::new();
    for (index, archetype_id) in chosen.iter().enumerate() {
        let def = &catalogue.npcs.archetypes[archetype_id];
        // The pool is indexed structurally and resolved for display: which
        // villager is drawn is generation, what they are called is text.
        let name = catalogue
            .strings
            .get(&def.name_pool[rng.index(def.name_pool.len())])
            .to_owned();
        let disposition = if hostile_index == Some(index) {
            Disposition::Hostile
        } else if rng.percent(50) {
            Disposition::Friendly
        } else {
            Disposition::Wary
        };
        let secret_template = def.secrets[rng.index(def.secrets.len())].clone();
        members.push(CastMember {
            archetype: archetype_id.clone(),
            name,
            disposition,
            mystical: def.mystical,
            // Non-mystical, non-hostile villagers will trade powder and ball.
            trades: !def.mystical && disposition != Disposition::Hostile,
            secret_template,
            is_host: host_index == Some(index),
        });
    }

    // Every cast member gets at least two links: the full triangle.
    let kinds = &catalogue.npcs.relationship_kinds;
    let mut links = Vec::new();
    for (a, b) in [(0usize, 1usize), (1, 2), (0, 2)] {
        let kind = &kinds[rng.index(kinds.len())];
        links.push((a, b, kind.id.clone()));
    }

    // The host must not be the hunter's only viable informant: require at
    // least one non-host, non-hostile cast member.
    let informants = members
        .iter()
        .filter(|member| !member.is_host && member.disposition != Disposition::Hostile)
        .count();
    if informants == 0 {
        return Err("cast has no viable informant".to_owned());
    }

    Ok(Cast { members, links })
}
