//! World materialisation: templates + cast + clue graph into a `World`.

use std::collections::BTreeMap;

use rh_content::{
    Catalogue, ClueCategory, ClueTemplate, Concealment, GatherDiscovery, MapTemplate,
    OpportunityAction, PoolKind, SiteKind, Terrain,
};
use rh_core::geometry::{Point, MAP_HEIGHT, MAP_WIDTH};
use rh_core::rng::SimRng;
use rh_core::world::{
    DiscoveryRule, Disposition, EnemySpawn, ExitSpec, FeatureId, FeatureKind, FeatureSpec,
    GraveContents, MapId, NpcId, NpcLink, NpcSecret, NpcSpec, OpportunityAnchor, OpportunityGrant,
    OpportunityId, OpportunitySpec, VillainSpec, World, WorldMap,
};

use crate::cast::{Cast, Combo};

/// Stable map order: settlement is always MapId(0). Which *template* fills
/// each role is chosen per run; the roles themselves never move.
use rh_content::MapRole;

pub fn build_world(
    seed: u64,
    catalogue: &Catalogue,
    combo: &Combo,
    cast: &Cast,
    ambush_percent: u8,
    rng: &mut SimRng,
) -> Result<World, String> {
    let mut builder = Builder {
        catalogue,
        combo,
        cast,
        rng,
        maps: Vec::new(),
        slot_index: BTreeMap::new(),
        chosen_templates: Vec::new(),
        npcs: Vec::new(),
        opportunities: Vec::new(),
        next_feature: 0,
        villain_grave: None,
        deceased_by_grave: BTreeMap::new(),
    };
    builder.build_maps()?;
    builder.place_npcs()?;
    let villain = builder.place_villain()?;
    builder.place_force_ops();
    let clue_ids = builder.place_clues(&villain)?;
    builder.place_gathers(&clue_ids)?;
    builder.place_scheme_preempt()?;
    builder.place_social_ops()?;

    Ok(World {
        seed,
        villain,
        maps: builder.maps,
        npcs: builder.npcs,
        opportunities: builder.opportunities,
        ambush_percent,
        certified_routes: Vec::new(),
        // Filled in at the acceptance point, once certification has said
        // whether a node must be banked.
        opening: rh_core::world::OpeningSituation {
            opening: String::new(),
            conditions: Vec::new(),
            prior: None,
        },
    })
}

struct Builder<'a> {
    catalogue: &'a Catalogue,
    combo: &'a Combo,
    cast: &'a Cast,
    rng: &'a mut SimRng,
    maps: Vec<WorldMap>,
    /// (role label, slot id) -> (MapId, position, site kind). Keyed by role
    /// rather than template, since content anchors name the kind of place and
    /// the template filling it varies per run.
    slot_index: BTreeMap<(String, String), (MapId, Point, SiteKind)>,
    /// Template chosen for each role this run, in `MapRole::ORDER`.
    chosen_templates: Vec<String>,
    npcs: Vec<NpcSpec>,
    opportunities: Vec<OpportunitySpec>,
    next_feature: u16,
    villain_grave: Option<(MapId, FeatureId, Point)>,
    deceased_by_grave: BTreeMap<FeatureId, String>,
}

impl<'a> Builder<'a> {
    /// Resolve the map a content anchor names ("settlement", "outlying", ...).
    fn map_id(&self, role_name: &str) -> MapId {
        let role = MapRole::from_content(role_name).unwrap_or(MapRole::Settlement);
        MapId(
            MapRole::ORDER
                .iter()
                .position(|candidate| *candidate == role)
                .unwrap_or(0) as u8,
        )
    }

    /// The English behind an authored id. `World` stores resolved text: it
    /// already holds fully-substituted prose, it is never persisted, and
    /// keeping ids here would force every UI call site to carry the
    /// substitution arguments too.
    fn text(&self, id: &rh_content::StringId) -> &str {
        self.catalogue.strings.get(id)
    }

    fn next_opportunity_id(&self) -> OpportunityId {
        OpportunityId(self.opportunities.len() as u16)
    }

    fn build_maps(&mut self) -> Result<(), String> {
        for role in MapRole::ORDER {
            // One template per role, drawn from the seed stream: the same seed
            // always dresses the valley the same way.
            let candidates = self.catalogue.templates_for(role);
            if candidates.is_empty() {
                return Err(format!("no map template for role '{}'", role.label()));
            }
            let pick = self.rng.below(candidates.len() as u32) as usize;
            let template_id = candidates[pick].clone();
            let template = self
                .catalogue
                .maps
                .get(&template_id)
                .ok_or_else(|| format!("missing map template '{template_id}'"))?;
            let map = self.build_map(role.label(), &template_id, template)?;
            self.chosen_templates.push(template_id);
            self.maps.push(map);
        }
        // Wire paired exits now every map exists.
        for index in 0..self.maps.len() {
            let exits = self.maps[index].exits.clone();
            for (exit_index, exit) in exits.iter().enumerate() {
                let dest = &self.maps[exit.to_map.0 as usize];
                let back = dest
                    .exits
                    .iter()
                    .find(|back| back.to_map.0 as usize == index)
                    .ok_or_else(|| {
                        format!(
                            "map '{}' has no exit back to '{}'",
                            dest.template, self.maps[index].template
                        )
                    })?;
                let to_point = back.at;
                self.maps[index].exits[exit_index].to_point = to_point;
            }
        }
        Ok(())
    }

    fn build_map(
        &mut self,
        role_label: &str,
        template_id: &str,
        template: &MapTemplate,
    ) -> Result<WorldMap, String> {
        let mut tiles = Vec::with_capacity((MAP_WIDTH * MAP_HEIGHT) as usize);
        for row in &template.rows {
            for glyph in row.chars() {
                let terrain = template
                    .legend
                    .get(&glyph)
                    .copied()
                    .ok_or_else(|| format!("glyph '{glyph}' missing from legend"))?;
                tiles.push(terrain);
            }
        }

        let map_id = self.map_id(role_label);
        let mut features = Vec::new();
        for slot in &template.slots {
            let at = Point::new(i16::from(slot.at[0]), i16::from(slot.at[1]));
            self.slot_index.insert(
                (role_label.to_owned(), slot.id.clone()),
                (map_id, at, slot.kind),
            );
            let feature_id = FeatureId(self.next_feature);
            match slot.kind {
                SiteKind::Grave => {
                    let deceased = self.pick_deceased();
                    self.deceased_by_grave.insert(feature_id, deceased.clone());
                    features.push(FeatureSpec {
                        id: feature_id,
                        at,
                        kind: FeatureKind::Grave {
                            contents: GraveContents::Mundane,
                        },
                        name: self
                            .catalogue
                            .strings
                            .ui_fill("gen.feature.grave", &[("deceased", &deceased)]),
                    });
                    self.next_feature += 1;
                }
                SiteKind::Workstation => {
                    features.push(FeatureSpec {
                        id: feature_id,
                        at,
                        kind: FeatureKind::Workstation,
                        name: self.catalogue.strings.ui("gen.feature.forge").to_owned(),
                    });
                    self.next_feature += 1;
                }
                SiteKind::Church if slot.id.contains("altar") => {
                    features.push(FeatureSpec {
                        id: feature_id,
                        at,
                        kind: FeatureKind::Altar,
                        name: self.catalogue.strings.ui("gen.feature.altar").to_owned(),
                    });
                    self.next_feature += 1;
                }
                _ => {
                    features.push(FeatureSpec {
                        id: feature_id,
                        at,
                        kind: if slot.kind == SiteKind::KillSite {
                            FeatureKind::KillSite
                        } else {
                            FeatureKind::Landmark
                        },
                        name: self.catalogue.strings.get(&slot.label).to_owned(),
                    });
                    self.next_feature += 1;
                }
            }
        }

        let exits = template
            .exits
            .iter()
            .map(|exit| ExitSpec {
                at: Point::new(i16::from(exit.at[0]), i16::from(exit.at[1])),
                to_map: self.map_id(&exit.to),
                // Fixed up in build_maps once all maps exist.
                to_point: Point::new(0, 0),
                ambush_route: is_ambush_leg(role_label, &exit.to),
            })
            .collect();

        // Consecration ward: the church interior, flood-filled from the altar
        // across floor tiles, stopping at walls and doors.
        let consecration_area = if role_label == "settlement" {
            let altar = features
                .iter()
                .find(|feature| feature.kind == FeatureKind::Altar)
                .map(|feature| feature.at);
            match altar {
                Some(altar) => flood_floor(&tiles, altar),
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let entry = closest_walkable(&tiles, Point::new(15, 10));

        let mut initial_enemies = Vec::new();
        for spawn in &template.initial_enemies {
            let (_, near, _) = self
                .slot_index
                .get(&(role_label.to_owned(), spawn.near_slot.clone()))
                .copied()
                .ok_or_else(|| format!("spawn slot '{}' missing", spawn.near_slot))?;
            let mut placed = 0;
            let mut ring: i16 = 1;
            while placed < spawn.count && ring <= 4 {
                for dy in -ring..=ring {
                    for dx in -ring..=ring {
                        if placed >= spawn.count {
                            break;
                        }
                        if dx.abs().max(dy.abs()) != ring {
                            continue;
                        }
                        let point = Point::new(near.x + dx, near.y + dy);
                        if point.in_bounds()
                            && walkable(tile_at(&tiles, point))
                            && !initial_enemies.iter().any(|e: &EnemySpawn| e.at == point)
                        {
                            initial_enemies.push(EnemySpawn {
                                enemy: spawn.enemy.clone(),
                                at: point,
                            });
                            placed += 1;
                        }
                    }
                }
                ring += 1;
            }
        }

        Ok(WorldMap {
            template: template_id.to_owned(),
            name: self.text(&template.name).to_owned(),
            role: template.role,
            tiles,
            exits,
            features,
            consecration_area,
            entry,
            initial_enemies,
        })
    }

    fn pick_deceased(&mut self) -> String {
        let pool = &self.catalogue.npcs.deceased_name_pool;
        let picked = self.rng.index(pool.len());
        self.catalogue.strings.get(&pool[picked]).to_owned()
    }

    fn place_npcs(&mut self) -> Result<(), String> {
        for (index, member) in self.cast.members.iter().enumerate() {
            let archetype = &self.catalogue.npcs.archetypes[&member.archetype];
            let (map, work, _) = self
                .slot_index
                .iter()
                .find(|((_, slot_id), _)| *slot_id == archetype.work_slot)
                .map(|(_, value)| *value)
                .ok_or_else(|| format!("work slot '{}' not found", archetype.work_slot))?;
            let secret_def = &self.catalogue.npcs.secrets[&member.secret_template];
            let links = self
                .cast
                .links
                .iter()
                .filter(|(a, b, _)| *a == index || *b == index)
                .map(|(a, b, kind_id)| {
                    let other = if *a == index { *b } else { *a };
                    let kind = self
                        .catalogue
                        .npcs
                        .relationship_kinds
                        .iter()
                        .find(|kind| kind.id == *kind_id)
                        .expect("validated relationship kind");
                    NpcLink {
                        to: NpcId(other as u8),
                        kind: kind_id.clone(),
                        discovered_text: self
                            .text(&kind.discovered_text)
                            .replace("{a}", &self.cast.members[*a].name)
                            .replace("{b}", &self.cast.members[*b].name),
                    }
                })
                .collect();
            self.npcs.push(NpcSpec {
                id: NpcId(index as u8),
                archetype: member.archetype.clone(),
                name: member.name.clone(),
                glyph: archetype.glyph,
                disposition: member.disposition,
                mystical: member.mystical,
                trades: member.trades,
                secret: NpcSecret {
                    template: member.secret_template.clone(),
                    text: self.text(&secret_def.text).replace("{npc}", &member.name),
                    disproof: secret_def
                        .disproof
                        .as_ref()
                        .map(|id| self.text(id).replace("{npc}", &member.name)),
                },
                links,
                map,
                work,
            });
        }
        Ok(())
    }

    fn place_villain(&mut self) -> Result<VillainSpec, String> {
        let villain_def = &self.catalogue.villains[&self.combo.villain];
        match villain_def.concealment {
            Concealment::NpcHost => {
                let host_index = self
                    .cast
                    .members
                    .iter()
                    .position(|member| member.is_host)
                    .ok_or_else(|| "no host in cast".to_owned())?;
                let host = NpcId(host_index as u8);
                // The beast dens in the deep wood.
                let den = self
                    .slot_index
                    .get(&("wilderness".to_owned(), "wolf-den".to_owned()))
                    .copied()
                    .map(|(map, at, _)| (map, at))
                    .unwrap_or((MapId(1), Point::new(10, 14)));
                Ok(VillainSpec {
                    archetype: self.combo.villain.clone(),
                    origin: self.combo.origin.clone(),
                    scheme: self.combo.scheme.clone(),
                    title: self.catalogue.strings.ui_fill(
                        "gen.villain.werewolf-title",
                        &[("npc", &self.cast.members[host_index].name)],
                    ),
                    host: Some(host),
                    grave: None,
                    lair: den,
                })
            }
            Concealment::DormantGrave => {
                // Origin picks the ground: old curses sleep by the church,
                // fresh wrongs are buried out at the manor crypt.
                let origin = &self.catalogue.origins[&self.combo.origin];
                let use_settlement = origin.sign_sites.contains(&SiteKind::Grave);
                let map_template = if use_settlement {
                    "settlement"
                } else {
                    "outlying"
                };
                let map_id = self.map_id(map_template);
                // Prefer graves reachable without forcing anything: gated
                // crypt niches stay optional content, never the mandatory
                // hunt site.
                let world_map = &self.maps[map_id.0 as usize];
                let entry = world_map.entry;
                let all_graves: Vec<(FeatureId, Point, String)> = world_map
                    .features
                    .iter()
                    .filter(|feature| matches!(feature.kind, FeatureKind::Grave { .. }))
                    .map(|feature| (feature.id, feature.at, feature.name.clone()))
                    .collect();
                let open_graves: Vec<(FeatureId, Point, String)> = all_graves
                    .iter()
                    .filter(|(_, at, _)| reachable(&world_map.tiles, entry, *at, None))
                    .cloned()
                    .collect();
                let graves = if open_graves.is_empty() {
                    all_graves
                } else {
                    open_graves
                };
                if graves.is_empty() {
                    return Err(format!("no graves on '{map_template}' for the villain"));
                }
                let (feature_id, at, name) = graves[self.rng.index(graves.len())].clone();
                let map = &mut self.maps[map_id.0 as usize];
                for feature in &mut map.features {
                    if feature.id == feature_id {
                        feature.kind = FeatureKind::Grave {
                            contents: GraveContents::Villain,
                        };
                    } else if let FeatureKind::Grave { contents } = &mut feature.kind {
                        // A few graves lie empty for unsettling texture.
                        if self.rng.percent(20) {
                            *contents = GraveContents::Empty;
                        }
                    }
                }
                self.villain_grave = Some((map_id, feature_id, at));
                Ok(VillainSpec {
                    archetype: self.combo.villain.clone(),
                    origin: self.combo.origin.clone(),
                    scheme: self.combo.scheme.clone(),
                    title: self
                        .catalogue
                        .strings
                        .ui_fill("gen.villain.revenant-title", &[("name", &name)]),
                    host: None,
                    grave: Some((map_id, feature_id)),
                    lair: (map_id, at),
                })
            }
        }
    }

    /// One force opportunity per forceable tile: visible Physical affordances.
    fn place_force_ops(&mut self) {
        for map_index in 0..self.maps.len() {
            let map_id = MapId(map_index as u8);
            for y in 0..MAP_HEIGHT {
                for x in 0..MAP_WIDTH {
                    let at = Point::new(x, y);
                    let terrain = tile_at(&self.maps[map_index].tiles, at);
                    let (name, prompt, reveal) = match terrain {
                        Terrain::Rubble => (
                            "gen.force.rubble.name",
                            "gen.force.rubble.prompt",
                            "gen.force.rubble.reveal",
                        ),
                        Terrain::BarredDoor => (
                            "gen.force.barred-door.name",
                            "gen.force.barred-door.prompt",
                            "gen.force.barred-door.reveal",
                        ),
                        _ => continue,
                    };
                    let id = self.next_opportunity_id();
                    self.opportunities.push(OpportunitySpec {
                        id,
                        source: "force".to_owned(),
                        name: self.catalogue.strings.ui(name).to_owned(),
                        map: map_id,
                        anchor: OpportunityAnchor::Tile(at),
                        pool: Some(PoolKind::Physical),
                        cost: 1,
                        obscurity: 0,
                        discovery: DiscoveryRule::Sight,
                        grants: OpportunityGrant::Lead,
                        requires: None,
                        clears_terrain: true,
                        covert: false,
                        prompt: self.catalogue.strings.ui(prompt).to_owned(),
                        reveal: self.catalogue.strings.ui(reveal).to_owned(),
                    });
                }
            }
        }
    }

    /// Instantiate clue templates into placed opportunities.
    /// Returns clue template id -> opportunity id for gather wiring.
    fn place_clues(
        &mut self,
        villain: &VillainSpec,
    ) -> Result<BTreeMap<String, OpportunityId>, String> {
        let fitting: Vec<(String, ClueTemplate)> = self
            .catalogue
            .clues
            .iter()
            .filter(|(_, template)| {
                template.fits(&self.combo.villain, &self.combo.origin, &self.combo.scheme)
            })
            .map(|(id, template)| (id.clone(), template.clone()))
            .collect();

        let mut chosen: Vec<(String, ClueTemplate)> = Vec::new();

        // Identity. Every case opens ambiguous: the soft signs go in first so
        // the early picture fits more than one answer, then the discriminators
        // that let the player actually close it. Two discriminators are
        // guaranteed by content validation; both are placed so losing one
        // informant never strands the case.
        let soft_identity: Vec<&(String, ClueTemplate)> = fitting
            .iter()
            .filter(|(_, t)| t.category == ClueCategory::Identity && !t.is_discriminating())
            .collect();
        let mut hard_identity: Vec<&(String, ClueTemplate)> = fitting
            .iter()
            .filter(|(_, t)| t.category == ClueCategory::Identity && t.is_discriminating())
            .collect();
        if hard_identity.len() < 2 {
            return Err(format!(
                "only {} discriminating identity clues fit the case",
                hard_identity.len()
            ));
        }
        for entry in &soft_identity {
            chosen.push((*entry).clone());
        }
        hard_identity.sort_by_key(|(id, t)| (t.obscurity, id.clone()));
        for entry in &hard_identity {
            chosen.push((*entry).clone());
        }

        // Origin and scheme signs. Both axes decide something concrete — the
        // reagent every counter is quenched with, and what can be pre-empted —
        // so each needs its discriminators placed and reachable.
        for (axis, category) in [
            ("origin", ClueCategory::OriginSign),
            ("scheme", ClueCategory::SchemeSign),
        ] {
            let soft: Vec<&(String, ClueTemplate)> = fitting
                .iter()
                .filter(|(_, t)| t.category == category && !t.is_discriminating())
                .collect();
            let mut hard: Vec<&(String, ClueTemplate)> = fitting
                .iter()
                .filter(|(_, t)| t.category == category && t.is_discriminating())
                .collect();
            if hard.len() < 2 {
                return Err(format!(
                    "only {} discriminating {axis} signs fit the case",
                    hard.len()
                ));
            }
            hard.sort_by_key(|(id, t)| (t.obscurity, id.clone()));
            for entry in soft.iter().chain(hard.iter()) {
                chosen.push((*entry).clone());
            }
        }

        // One location clue.
        let locations: Vec<&(String, ClueTemplate)> = fitting
            .iter()
            .filter(|(_, template)| template.category == ClueCategory::Location)
            .collect();
        if locations.is_empty() {
            return Err("no location clue fits the case".to_owned());
        }
        chosen.push(locations[self.rng.index(locations.len())].clone());

        // Every fitting weakness / ingredient-source clue.
        for entry in fitting.iter().filter(|(_, template)| {
            matches!(
                template.category,
                ClueCategory::Weakness | ClueCategory::IngredientSource
            )
        }) {
            chosen.push(entry.clone());
        }

        let mut placed = BTreeMap::new();
        for (template_id, template) in chosen {
            let id = self.place_clue(&template_id, &template, villain)?;
            placed.insert(template_id, id);
        }
        Ok(placed)
    }

    fn place_clue(
        &mut self,
        template_id: &str,
        template: &ClueTemplate,
        villain: &VillainSpec,
    ) -> Result<OpportunityId, String> {
        let host_name = villain
            .host
            .map(|id| self.npcs[id.0 as usize].name.clone())
            .unwrap_or_default();
        let grave_name = self
            .villain_grave
            .as_ref()
            .and_then(|(map, feature, _)| {
                self.maps[map.0 as usize]
                    .features
                    .iter()
                    .find(|f| f.id == *feature)
                    .map(|f| f.name.clone())
            })
            .unwrap_or_default();
        let fill = |text: &str| {
            text.replace("{npc}", &host_name)
                .replace("{grave}", &grave_name)
        };

        let (map, anchor, requires) = self.clue_anchor(template, villain)?;
        let discriminating = template.is_discriminating();
        let grants = match template.category {
            ClueCategory::Identity => OpportunityGrant::IdentityClue { discriminating },
            ClueCategory::OriginSign => OpportunityGrant::OriginSign { discriminating },
            ClueCategory::SchemeSign => OpportunityGrant::SchemeSign { discriminating },
            ClueCategory::Location => OpportunityGrant::LocationClue,
            ClueCategory::Weakness | ClueCategory::IngredientSource => {
                if template.grants_items.is_empty() {
                    OpportunityGrant::Lead
                } else {
                    OpportunityGrant::Items {
                        items: template.grants_items.clone(),
                    }
                }
            }
        };
        let id = self.next_opportunity_id();
        self.opportunities.push(OpportunitySpec {
            id,
            source: template_id.to_owned(),
            name: self.text(&template.name).to_owned(),
            map,
            anchor,
            pool: Some(template.pool),
            cost: 1,
            obscurity: template.obscurity,
            discovery: DiscoveryRule::Sight,
            grants,
            requires,
            clears_terrain: false,
            covert: matches!(
                template.action,
                OpportunityAction::Spy | OpportunityAction::Examine | OpportunityAction::Track
            ),
            prompt: fill(self.text(&template.prompt)),
            reveal: fill(self.text(&template.reveal)),
        });
        Ok(id)
    }

    /// Resolve a clue template's abstract site into a concrete anchor.
    fn clue_anchor(
        &mut self,
        template: &ClueTemplate,
        villain: &VillainSpec,
    ) -> Result<(MapId, OpportunityAnchor, Option<OpportunityId>), String> {
        let slot = |builder: &Self, map: &str, slot: &str| -> Option<(MapId, Point)> {
            builder
                .slot_index
                .get(&(map.to_owned(), slot.to_owned()))
                .map(|(id, at, _)| (*id, *at))
        };
        match template.site {
            SiteKind::KillSite => {
                let (map, at) = slot(self, "settlement", "kill-site")
                    .ok_or_else(|| "kill-site slot missing".to_owned())?;
                Ok((map, OpportunityAnchor::Tile(at), None))
            }
            SiteKind::Church => {
                let slot_id = template
                    .church_slot
                    .ok_or_else(|| "church clue without a church_slot".to_owned())?
                    .slot_id();
                let (map, at) = slot(self, "settlement", slot_id)
                    .ok_or_else(|| format!("church slot '{slot_id}' missing"))?;
                Ok((map, OpportunityAnchor::Tile(at), None))
            }
            SiteKind::Wilds => {
                let slot_id = match template.action {
                    OpportunityAction::Commune => "standing-stones",
                    _ => "ambush-site",
                };
                let (map, at) = slot(self, "wilderness", slot_id)
                    .ok_or_else(|| format!("wilds slot '{slot_id}' missing"))?;
                Ok((map, OpportunityAnchor::Tile(at), None))
            }
            SiteKind::OutlyingSite => {
                let slot_id = match template.action {
                    OpportunityAction::Commune => "manor-hall",
                    OpportunityAction::Force => "manor-cellar",
                    _ => "manor-study",
                };
                let (map, at) = slot(self, "outlying", slot_id)
                    .ok_or_else(|| format!("outlying slot '{slot_id}' missing"))?;
                let requires = self.access_gate(map, at);
                Ok((map, OpportunityAnchor::Tile(at), requires))
            }
            SiteKind::Grave => {
                // Evidence that points at the villain sits on the villain's
                // own grave when it has one. Everything else — and every case
                // whose villain walks about on two legs — uses the ordinary
                // consecrated rows in town.
                let points_at_villain = matches!(
                    template.category,
                    ClueCategory::Identity | ClueCategory::Location
                );
                match (points_at_villain, self.villain_grave) {
                    (true, Some((map, _, at))) => {
                        let requires = self.access_gate(map, at);
                        Ok((map, OpportunityAnchor::Tile(at), requires))
                    }
                    _ => {
                        let candidates: Vec<Point> = self.maps[0]
                            .features
                            .iter()
                            .filter(|feature| {
                                matches!(
                                    feature.kind,
                                    FeatureKind::Grave {
                                        contents: GraveContents::Mundane
                                    }
                                )
                            })
                            .map(|feature| feature.at)
                            .collect();
                        if candidates.is_empty() {
                            return Err("no mundane settlement grave for weakness clue".to_owned());
                        }
                        let at = candidates[self.rng.index(candidates.len())];
                        Ok((MapId(0), OpportunityAnchor::Tile(at), None))
                    }
                }
            }
            SiteKind::Npc => {
                let anchor_npc = match template.action {
                    // Watching or examining the suspect directly.
                    OpportunityAction::Spy | OpportunityAction::Examine
                        if villain.host.is_some() =>
                    {
                        villain.host.ok_or_else(|| "host missing".to_owned())?
                    }
                    // Asking an informant about the villain.
                    _ => {
                        let informants: Vec<NpcId> = self
                            .npcs
                            .iter()
                            .filter(|npc| {
                                Some(npc.id) != villain.host
                                    && npc.disposition != Disposition::Hostile
                            })
                            .map(|npc| npc.id)
                            .collect();
                        if informants.is_empty() {
                            return Err("no eligible informant".to_owned());
                        }
                        informants[self.rng.index(informants.len())]
                    }
                };
                let map = self.npcs[anchor_npc.0 as usize].map;
                Ok((map, OpportunityAnchor::Npc(anchor_npc), None))
            }
            SiteKind::Workstation => {
                let (map, at) = slot(self, "settlement", "forge")
                    .ok_or_else(|| "forge slot missing".to_owned())?;
                Ok((map, OpportunityAnchor::Tile(at), None))
            }
        }
    }

    /// If reaching `at` requires clearing a forceable tile, return that
    /// force opportunity so the planner schedules it first.
    fn access_gate(&self, map: MapId, at: Point) -> Option<OpportunityId> {
        let tiles = &self.maps[map.0 as usize].tiles;
        let entry = self.maps[map.0 as usize].entry;
        if reachable(tiles, entry, at, None) {
            return None;
        }
        // Try each force opportunity on this map as the unlocking gate.
        for opp in &self.opportunities {
            if opp.map != map || !opp.clears_terrain {
                continue;
            }
            if let OpportunityAnchor::Tile(gate) = opp.anchor {
                if reachable(tiles, entry, at, Some(gate)) {
                    return Some(opp.id);
                }
            }
        }
        None
    }

    fn place_gathers(&mut self, clue_ids: &BTreeMap<String, OpportunityId>) -> Result<(), String> {
        for (gather_id, gather) in &self.catalogue.gathers {
            let (map, at, _) = self
                .slot_index
                .get(&(gather.map.clone(), gather.slot.clone()))
                .copied()
                .ok_or_else(|| format!("gather '{gather_id}' slot missing"))?;
            let discovery = match &gather.discovery {
                GatherDiscovery::Sight => DiscoveryRule::Sight,
                GatherDiscovery::RevealedByClue { clue } => match clue_ids.get(clue) {
                    Some(op) => DiscoveryRule::RevealedBy(*op),
                    // Its revealing clue is not in this run: skip the gather.
                    None => continue,
                },
                GatherDiscovery::SightOrClue { clue } => match clue_ids.get(clue) {
                    Some(op) => DiscoveryRule::SightOr(*op),
                    None => DiscoveryRule::Sight,
                },
            };
            let requires = self.access_gate(map, at);
            let id = self.next_opportunity_id();
            self.opportunities.push(OpportunitySpec {
                id,
                source: gather_id.clone(),
                name: self.text(&gather.name).to_owned(),
                map,
                anchor: OpportunityAnchor::Tile(at),
                pool: gather.pool,
                cost: gather.cost,
                obscurity: 0,
                discovery,
                grants: OpportunityGrant::Items {
                    items: gather.items.clone(),
                },
                requires,
                clears_terrain: false,
                covert: false,
                prompt: self.text(&gather.prompt).to_owned(),
                reveal: self.text(&gather.reveal).to_owned(),
            });
        }
        Ok(())
    }

    /// Place the one act that blunts this scheme before its major event.
    /// It is always optional — no certified route depends on it — but reading
    /// the scheme tells the player it exists and where.
    fn place_scheme_preempt(&mut self) -> Result<(), String> {
        let scheme = self.catalogue.schemes[&self.combo.scheme].clone();
        let preempt = &scheme.preempt;
        // Find a slot of the required kind on a map of the required role.
        let target = self
            .maps
            .iter()
            .enumerate()
            .filter(|(_, map)| map.role == preempt.map_role)
            .find_map(|(index, map)| {
                let map_id = MapId(index as u8);
                self.slot_index
                    .iter()
                    .find(|((template, _), (slot_map, _, kind))| {
                        // Anchors are keyed by role, not by the template drawn
                        // to fill it: comparing against the template id here
                        // silently found nothing whenever a role was filled by
                        // anything but the template named after it.
                        *template == map.role.label()
                            && *slot_map == map_id
                            && *kind == preempt.site
                    })
                    .map(|(_, (_, at, _))| (map_id, *at))
            });
        let Some((map, at)) = target else {
            return Err(format!(
                "scheme '{}' pre-emption has no {:?} site on a {:?} map",
                self.combo.scheme, preempt.site, preempt.map_role
            ));
        };
        let requires = self.access_gate(map, at);
        let id = self.next_opportunity_id();
        self.opportunities.push(OpportunitySpec {
            id,
            source: format!("preempt:{}", self.combo.scheme),
            name: self.text(&preempt.name).to_owned(),
            map,
            anchor: OpportunityAnchor::Tile(at),
            pool: Some(preempt.pool),
            cost: preempt.cost,
            obscurity: 1,
            discovery: DiscoveryRule::Sight,
            grants: OpportunityGrant::SchemePreempt,
            requires,
            clears_terrain: false,
            covert: false,
            prompt: self.text(&preempt.prompt).to_owned(),
            reveal: self.text(&preempt.reveal).to_owned(),
        });
        Ok(())
    }

    fn place_social_ops(&mut self) -> Result<(), String> {
        for index in 0..self.npcs.len() {
            let npc = self.npcs[index].clone();
            let map = npc.map;
            let secret_def = &self.catalogue.npcs.secrets[&npc.secret.template];

            // Watch them quietly: learn their secret.
            let spy_id = self.next_opportunity_id();
            self.opportunities.push(OpportunitySpec {
                id: spy_id,
                source: format!("spy:{}", npc.name),
                name: self
                    .catalogue
                    .strings
                    .ui_fill("gen.npc.spy.name", &[("npc", &npc.name)]),
                map,
                anchor: OpportunityAnchor::Npc(npc.id),
                pool: Some(PoolKind::Social),
                cost: 1,
                obscurity: 1,
                discovery: DiscoveryRule::Sight,
                grants: OpportunityGrant::SecretInfo,
                requires: None,
                clears_terrain: false,
                covert: true,
                prompt: self
                    .catalogue
                    .strings
                    .ui_fill("gen.npc.spy.prompt", &[("npc", &npc.name)]),
                reveal: self
                    .catalogue
                    .strings
                    .ui_fill("gen.npc.spy.reveal", &[("npc", &npc.name)]),
            });

            if secret_def.false_secret {
                // The false secret must have reachable disproof: the records.
                let (records_map, records_at, _) = self
                    .slot_index
                    .get(&("settlement".to_owned(), "church-records".to_owned()))
                    .copied()
                    .ok_or_else(|| "church-records slot missing".to_owned())?;
                let id = self.next_opportunity_id();
                self.opportunities.push(OpportunitySpec {
                    id,
                    source: format!("disproof:{}", npc.name),
                    name: self
                        .catalogue
                        .strings
                        .ui_fill("gen.npc.disproof.name", &[("npc", &npc.name)]),
                    map: records_map,
                    anchor: OpportunityAnchor::Tile(records_at),
                    pool: Some(PoolKind::Lore),
                    cost: 1,
                    obscurity: 1,
                    discovery: DiscoveryRule::RevealedBy(spy_id),
                    grants: OpportunityGrant::Disproof { npc: npc.id },
                    requires: None,
                    clears_terrain: false,
                    covert: true,
                    prompt: self
                        .catalogue
                        .strings
                        .ui_fill("gen.npc.disproof.prompt", &[("npc", &npc.name)]),
                    reveal: self
                        .catalogue
                        .strings
                        .ui("gen.npc.disproof.reveal")
                        .to_owned(),
                });
            } else {
                // A true secret is leverage.
                let id = self.next_opportunity_id();
                self.opportunities.push(OpportunitySpec {
                    id,
                    source: format!("expose:{}", npc.name),
                    name: self
                        .catalogue
                        .strings
                        .ui_fill("gen.npc.expose.name", &[("npc", &npc.name)]),
                    map,
                    anchor: OpportunityAnchor::Npc(npc.id),
                    pool: Some(PoolKind::Social),
                    cost: 1,
                    obscurity: 1,
                    discovery: DiscoveryRule::RevealedBy(spy_id),
                    grants: OpportunityGrant::Leverage,
                    requires: None,
                    clears_terrain: false,
                    covert: false,
                    prompt: format!(
                        "What you know about {} would loosen their tongue.",
                        npc.name
                    ),
                    reveal: String::new(),
                });
            }

            // Ask around about their entanglements.
            let id = self.next_opportunity_id();
            self.opportunities.push(OpportunitySpec {
                id,
                source: format!("ties:{}", npc.name),
                name: self
                    .catalogue
                    .strings
                    .ui_fill("gen.npc.gossip.name", &[("npc", &npc.name)]),
                map,
                anchor: OpportunityAnchor::Npc(npc.id),
                pool: Some(PoolKind::Social),
                cost: 1,
                obscurity: 0,
                discovery: DiscoveryRule::Sight,
                grants: OpportunityGrant::RelationshipInfo,
                requires: None,
                clears_terrain: false,
                covert: false,
                prompt: self
                    .catalogue
                    .strings
                    .ui_fill("gen.npc.gossip.prompt", &[("npc", &npc.name)]),
                reveal: String::new(),
            });

            // The mystical favour route.
            if npc.mystical {
                let id = self.next_opportunity_id();
                self.opportunities.push(OpportunitySpec {
                    id,
                    source: "mystic-favour".to_owned(),
                    name: self
                        .catalogue
                        .strings
                        .ui_fill("gen.npc.favour.name", &[("npc", &npc.name)]),
                    map,
                    anchor: OpportunityAnchor::Npc(npc.id),
                    pool: Some(PoolKind::Social),
                    cost: 1,
                    obscurity: 1,
                    discovery: DiscoveryRule::Sight,
                    grants: OpportunityGrant::MysticFavour,
                    requires: None,
                    clears_terrain: false,
                    covert: false,
                    prompt: self
                        .catalogue
                        .strings
                        .ui_fill("gen.npc.favour.prompt", &[("npc", &npc.name)]),
                    reveal: String::new(),
                });
            }
        }
        Ok(())
    }
}

fn is_ambush_leg(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("wilderness", "outlying") | ("outlying", "wilderness")
    )
}

fn tile_at(tiles: &[Terrain], at: Point) -> Terrain {
    if at.in_bounds() {
        tiles[at.y as usize * MAP_WIDTH as usize + at.x as usize]
    } else {
        Terrain::Wall
    }
}

fn walkable(terrain: Terrain) -> bool {
    rh_core::fov::is_walkable(terrain)
}

fn closest_walkable(tiles: &[Terrain], target: Point) -> Point {
    let mut best = target;
    let mut best_distance = i16::MAX;
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let point = Point::new(x, y);
            if walkable(tile_at(tiles, point)) {
                let distance = point.distance(target);
                if distance < best_distance {
                    best_distance = distance;
                    best = point;
                }
            }
        }
    }
    best
}

/// Church interior: flood fill floor/altar tiles from the altar, stopping at
/// anything else (walls and doors bound the ward).
fn flood_floor(tiles: &[Terrain], from: Point) -> Vec<Point> {
    let mut seen = vec![false; (MAP_WIDTH * MAP_HEIGHT) as usize];
    let mut area = Vec::new();
    let mut queue = vec![from];
    while let Some(point) = queue.pop() {
        if !point.in_bounds() {
            continue;
        }
        let index = point.y as usize * MAP_WIDTH as usize + point.x as usize;
        if seen[index] {
            continue;
        }
        seen[index] = true;
        let terrain = tile_at(tiles, point);
        if !matches!(terrain, Terrain::Floor | Terrain::Altar) {
            continue;
        }
        area.push(point);
        for neighbour in point.neighbours() {
            queue.push(neighbour);
        }
    }
    area.sort_by_key(|point| (point.y, point.x));
    area
}

/// Walkability search treating `unlocked` as cleared terrain.
fn reachable(tiles: &[Terrain], from: Point, to: Point, unlocked: Option<Point>) -> bool {
    if from == to {
        return true;
    }
    let mut seen = vec![false; (MAP_WIDTH * MAP_HEIGHT) as usize];
    let mut queue = vec![from];
    while let Some(point) = queue.pop() {
        if point == to || point.is_adjacent(to) {
            return true;
        }
        for next in point.neighbours() {
            if !next.in_bounds() {
                continue;
            }
            let index = next.y as usize * MAP_WIDTH as usize + next.x as usize;
            if seen[index] {
                continue;
            }
            let terrain = tile_at(tiles, next);
            let passable = walkable(terrain) || Some(next) == unlocked;
            if passable {
                seen[index] = true;
                queue.push(next);
            }
        }
    }
    false
}
