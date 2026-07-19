//! Serde schema for the authored TOML content files.
//!
//! Every struct uses `deny_unknown_fields` so a typo in a content file fails
//! validation instead of silently vanishing. All gameplay numbers live here
//! rather than as code constants, per the authored-content-catalogue spec.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::strings::StringId;

/// A grid coordinate inside a 32x20 tactical map, `[x, y]` with `0,0` top-left.
pub type Coord = [u8; 2];

// ---------------------------------------------------------------------------
// balance.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Balance {
    pub combat: CombatBalance,
    pub clock: ClockBalance,
    pub loot: LootBalance,
    pub generator: GeneratorBalance,
    pub vision: VisionBalance,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CombatBalance {
    /// Base chance to hit with a melee attack, in whole percent.
    pub melee_hit_percent: u8,
    /// Base chance to hit with a ranged attack, in whole percent.
    pub ranged_hit_percent: u8,
    /// Percentage points subtracted from a trapped enemy's attacks.
    pub trapped_attack_penalty_percent: u8,
    /// Percentage-point bonus for an attack made immediately after a pounce.
    pub pounce_attack_bonus_percent: u8,
    /// Encounter turns an enemy stays held in a snare.
    pub snare_hold_turns: u8,
    /// Damage the Occultist's marked ground deals to unnatural crossers.
    pub ground_ward_damage: u16,
    /// Stamina restored at the start of each hunter local turn.
    pub stamina_regen_per_turn: u8,
    /// Health fraction (percent) at or below which Killing Blow is enabled.
    pub killing_blow_health_percent: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClockBalance {
    /// Global travel turns before the final hunt begins.
    pub travel_turns: u8,
    /// Global turn on which the scheme's minor event fires.
    pub minor_event_turn: u8,
    /// Global turn on which the scheme's major event fires.
    pub major_event_turn: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LootBalance {
    /// Chance in whole percent that an ordinary enemy drops loot on death.
    pub drop_percent: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorBalance {
    /// Latest global turn by which the early hunt-ready route must complete.
    pub early_route_deadline: u8,
    /// Latest global turn by which the obvious fallback route must complete.
    pub fallback_route_deadline: u8,
    /// Maximum obscurity total allowed on the fallback route.
    pub fallback_obscurity_budget: u16,
    /// Maximum weighted effort cost allowed on a certified route.
    pub route_effort_budget: u16,
    /// Maximum travel legs allowed on a certified route.
    pub route_travel_budget: u8,
    /// Minimum planner-estimated final-fight viability, in permille.
    pub viability_threshold_permille: u16,
    /// Inclusive range for the seed-defined wilderness ambush chance, percent.
    pub ambush_percent_min: u8,
    pub ambush_percent_max: u8,
    /// Cover pockets each generated map must reserve.
    pub min_cover_pockets_per_map: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VisionBalance {
    /// Hunter field-of-view radius in tiles on tactical maps.
    pub fov_radius: u8,
}

// ---------------------------------------------------------------------------
// hunter.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HunterDef {
    pub name: StringId,
    /// One line placing this hunter in the valley, shown at selection.
    pub title: StringId,
    pub glyph: char,
    pub health: u16,
    pub lore_cap: u8,
    pub social_cap: u8,
    pub mystic_cap: u8,
    pub physical_cap: u8,
    pub stamina_cap: u8,
    /// Item ids granted at the start of a run (duplicates allowed).
    pub starting_items: Vec<String>,
    pub manoeuvres: Vec<ManoeuvreDef>,
    pub signatures: Vec<SignatureDef>,
}

/// Generic stamina manoeuvre shared by all future hunters.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManoeuvreDef {
    pub id: String,
    pub name: StringId,
    pub stamina_cost: u8,
    pub effect: ManoeuvreEffect,
    pub description: StringId,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ManoeuvreEffect {
    /// Next ranged attack this encounter always hits.
    SureRangedShot,
    /// Next melee attack deals damage multiplied by `numerator`/2 (x1.5 = 3).
    MeleeDamageMultiplier { numerator: u8 },
    /// Move `tiles` tiles in one action.
    Dash { tiles: u8 },
}

/// Hunter-specific signature ability fuelled by Physical points.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureDef {
    pub id: String,
    pub name: StringId,
    pub physical_cost: u8,
    pub effect: SignatureEffect,
    pub description: StringId,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SignatureEffect {
    /// Place a snare on an adjacent tile; first enemy entering is held.
    SetSnare,
    /// Double-damage melee attack against an immobile or wounded enemy.
    KillingBlow,
    /// Reinterpret one ambiguous identity sign already held, turning it into
    /// a discriminating proof. The Occultist's route through the evidence.
    ReadTheSign,
    /// Mark the ground around the hunter: unnatural things crossing it are
    /// torn at for the crossing.
    WardTheGround { turns: u8, radius: u8 },
}

// ---------------------------------------------------------------------------
// enemies.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnemyDef {
    pub name: StringId,
    pub glyph: char,
    pub health: u16,
    pub melee_damage: u16,
    /// Attack hit chance in whole percent.
    pub hit_percent: u8,
    pub behaviour: EnemyBehaviour,
    /// Whether this thing is unnatural: warded ground bites it, where a wolf
    /// or a hired knife walks across untroubled.
    #[serde(default)]
    pub unnatural: bool,
    /// Optional ranged attack (bandits).
    pub ranged: Option<EnemyRanged>,
    pub description: StringId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnemyBehaviour {
    /// Direct chase, full speed (wolves).
    PackHunter,
    /// Keeps distance and shoots when it can (bandits).
    Skirmisher,
    /// Moves only every other encounter turn (restless dead).
    Shambler,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnemyRanged {
    pub damage: u16,
    pub hit_percent: u8,
    pub range: u8,
}

// ---------------------------------------------------------------------------
// villains.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VillainDef {
    pub name: StringId,
    pub glyph: char,
    pub health: u16,
    pub melee_damage: u16,
    pub hit_percent: u8,
    /// Extra health granted per threat tier.
    pub tier_bonus_health: u16,
    /// Enhanced behaviours unlocked at threat tiers 1 and 2, in order.
    pub tier_behaviours: Vec<TierBehaviour>,
    /// Where this villain hides: an NPC host or a dormant grave.
    pub concealment: Concealment,
    pub pounce: Option<PounceDef>,
    pub regeneration: Option<RegenerationDef>,
    pub cadence: Option<CadenceDef>,
    /// A hex-ward that soaks blows until broken (the Witch).
    pub ward: Option<WardDef>,
    /// Item id that acts as this villain's decisive weakness counter.
    pub weakness_item: String,
    /// Whether the church consecration rite affects this villain.
    pub affected_by_consecration: bool,
    pub description: StringId,
}

/// A standing hex-ward: it soaks a number of blows before collapsing, and
/// the villain's weakness item cuts straight through it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WardDef {
    /// Blows the ward absorbs before it breaks.
    pub charges: u8,
    /// Damage that still leaks through each warded blow.
    pub leak_damage: u16,
    /// Encounter turns after breaking before the ward is rewoven.
    pub reweave_turns: u8,
    pub absorb_telegraph: StringId,
    pub break_telegraph: StringId,
    pub reweave_telegraph: StringId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TierBehaviour {
    pub id: String,
    pub name: StringId,
    pub effect: TierEffect,
    /// Event-log telegraph shown when the tier activates.
    pub telegraph: StringId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TierEffect {
    PounceCooldown {
        turns: u8,
    },
    DashCooldown {
        turns: u8,
    },
    BonusMeleeDamage {
        amount: u16,
    },
    /// Additional hex-ward charges (the Witch).
    WardCharges {
        amount: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Concealment {
    /// The villain is secretly one of the generated NPCs.
    NpcHost,
    /// The villain lies dormant in a generated grave.
    DormantGrave,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PounceDef {
    /// Maximum pounce distance along a clear line of sight.
    pub range: u8,
    /// Encounter turns between pounces.
    pub cooldown: u8,
    pub telegraph: StringId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegenerationDef {
    /// Health regained per encounter turn until stopped by the weakness item.
    pub per_turn: u16,
    pub telegraph: StringId,
}

/// Revenant-style shared vulnerability/dash cadence.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CadenceDef {
    /// Length of the cadence cycle in encounter turns.
    pub period: u8,
    /// Tiles moved by a dash in a straight line.
    pub dash_tiles: u8,
    /// Consecutive vulnerable turns forced by the binding counter item.
    pub bound_vulnerable_turns: u8,
    /// Ongoing damage per turn while on consecrated ground.
    pub consecrated_damage_per_turn: u16,
    pub vulnerable_telegraph: StringId,
    pub dash_telegraph: StringId,
    pub guarded_telegraph: StringId,
}

// ---------------------------------------------------------------------------
// origins.toml / schemes.toml
// ---------------------------------------------------------------------------

/// An origin changes the villain's signs and, decisively, which reagent the
/// villain's counter must be quenched with. Misreading the origin means
/// crafting a counter that will not bite.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OriginDef {
    pub name: StringId,
    /// Clue-site kinds this origin emphasises when placing identity clues.
    pub sign_sites: Vec<SiteKind>,
    /// Item id every decisive counter recipe additionally requires in a case
    /// of this origin. This is what makes reading the origin load-bearing.
    pub counter_reagent: String,
    /// Shown when a counter is crafted with this origin's reagent.
    pub counter_flavour: StringId,
    pub description: StringId,
}

/// A scheme controls the timed events, the minion family, and the one
/// pre-emption that can blunt its escalation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemeDef {
    pub name: StringId,
    /// Enemy id of the minion family this scheme fields.
    pub minion_enemy: String,
    pub minor_event: SchemeEvent,
    pub major_event: SchemeEvent,
    /// The act that, taken before the major event, blunts this scheme.
    pub preempt: SchemePreempt,
    pub description: StringId,
}

/// Disrupting a scheme before its major event: an authored, placeable act
/// that suppresses the major event's escalation when done in time.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemePreempt {
    pub name: StringId,
    /// Where the act can be performed.
    pub site: SiteKind,
    /// Map template role the site must belong to.
    pub map_role: MapRole,
    pub pool: PoolKind,
    pub cost: u8,
    pub prompt: StringId,
    pub reveal: StringId,
    /// Logged when the major event fires already blunted.
    pub blunted_text: StringId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemeEvent {
    pub name: StringId,
    /// Event-log text when the event fires.
    pub text: StringId,
    /// Map id on which the event leaves a visible mark (kill site, etc.).
    pub site_map: String,
    /// Minions spawned on the marked map when the event fires.
    pub spawn_minions: u8,
}

// ---------------------------------------------------------------------------
// items.toml / recipes.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    pub name: StringId,
    pub glyph: char,
    pub kind: ItemKind,
    pub description: StringId,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ItemKind {
    MeleeWeapon {
        damage: u16,
    },
    RangedWeapon {
        damage: u16,
        range: u8,
        ammo: String,
    },
    Ammunition,
    /// Special ammunition that also carries a weakness payload.
    WeaknessAmmunition {
        damage: u16,
        stops_regeneration: bool,
    },
    /// A melee counter that cuts straight through a hex-ward (cold iron).
    WeaknessBlade {
        damage: u16,
    },
    /// Consumable drink; restores health, consumes the encounter action.
    Draught {
        heal: u16,
    },
    /// Consumable used adjacent to a villain with a cadence (binding charm).
    BindingCharm,
    Ingredient,
    Coin,
    /// Craftable but useless in the hunt; texture and red herrings.
    Curio,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDef {
    pub name: StringId,
    /// Item ids consumed (duplicates allowed for quantities).
    pub inputs: Vec<String>,
    /// Item id produced.
    pub output: String,
    /// Decisive counters must additionally be quenched with the reagent of
    /// the case's origin, so the recipe cannot be completed until the origin
    /// has been read correctly.
    #[serde(default)]
    pub requires_origin_reagent: bool,
    pub description: StringId,
}

// ---------------------------------------------------------------------------
// clues.toml
// ---------------------------------------------------------------------------

/// Template for a generated clue opportunity.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClueTemplate {
    pub name: StringId,
    pub category: ClueCategory,
    /// Evidence claim on this category's axis: the values the sign is
    /// consistent with. Empty means "consistent with anything on the axis".
    /// This also scopes placement: the case's actual value must be listed.
    #[serde(default)]
    pub supports: Vec<String>,
    /// Values on this category's axis that the clue positively eliminates.
    /// A non-empty list makes the clue *discriminating*; certified routes
    /// require at least one discriminating identity clue.
    #[serde(default)]
    pub rules_out: Vec<String>,
    /// Cross-axis placement scoping (empty means any). Use these when a clue
    /// only makes sense in, say, a Witch case, without claiming anything
    /// about its own axis.
    #[serde(default)]
    pub villains: Vec<String>,
    #[serde(default)]
    pub origins: Vec<String>,
    #[serde(default)]
    pub schemes: Vec<String>,
    pub action: OpportunityAction,
    pub pool: PoolKind,
    /// Site kind where the generator may place this clue.
    pub site: SiteKind,
    /// Which church slot to anchor to. Required when `site` is `Church`,
    /// meaningless otherwise; `validate` enforces both.
    #[serde(default)]
    pub church_slot: Option<ChurchSlot>,
    /// 0 = obvious .. 3 = niche. Fallback routes prefer low totals.
    pub obscurity: u8,
    /// Items granted directly on resolution (weakness clues that hand over
    /// ingredients, like freely-given grave-dust). Usually empty.
    #[serde(default)]
    pub grants_items: Vec<String>,
    /// Opportunity text shown before the clue is taken.
    pub prompt: StringId,
    /// Event-log / journal text once revealed.
    pub reveal: StringId,
}

impl ClueTemplate {
    /// A clue that eliminates at least one alternative on its own axis.
    pub fn is_discriminating(&self) -> bool {
        !self.rules_out.is_empty()
    }

    /// Whether this clue may appear in a case with the given composition.
    pub fn fits(&self, villain: &str, origin: &str, scheme: &str) -> bool {
        let scoped =
            |list: &[String], value: &str| list.is_empty() || list.iter().any(|v| v == value);
        if !scoped(&self.villains, villain)
            || !scoped(&self.origins, origin)
            || !scoped(&self.schemes, scheme)
        {
            return false;
        }
        // The claim on its own axis must be true of this case.
        match self.category.axis() {
            None => true,
            Some(axis) => {
                let actual = match axis {
                    EvidenceAxis::Villain => villain,
                    EvidenceAxis::Origin => origin,
                    EvidenceAxis::Scheme => scheme,
                };
                scoped(&self.supports, actual) && !self.rules_out.iter().any(|v| v == actual)
            }
        }
    }
}

/// The three axes a case is composed on. Evidence speaks to exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceAxis {
    Villain,
    Origin,
    Scheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClueCategory {
    /// Corroborating identity evidence; speaks to the villain axis.
    Identity,
    /// A sign of how the evil began; speaks to the origin axis.
    OriginSign,
    /// A sign of what the evil is working toward; speaks to the scheme axis.
    SchemeSign,
    /// Reveals where the villain rests or who hosts it.
    Location,
    /// Reveals a specific weakness preparation (e.g. the candles are silver).
    Weakness,
    /// Grants crafting ingredients.
    IngredientSource,
}

impl ClueCategory {
    /// The case axis this category makes claims about, if any.
    pub fn axis(self) -> Option<EvidenceAxis> {
        match self {
            ClueCategory::Identity => Some(EvidenceAxis::Villain),
            ClueCategory::OriginSign => Some(EvidenceAxis::Origin),
            ClueCategory::SchemeSign => Some(EvidenceAxis::Scheme),
            ClueCategory::Location | ClueCategory::Weakness | ClueCategory::IngredientSource => {
                None
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpportunityAction {
    Examine,
    Track,
    Gossip,
    Persuade,
    Spy,
    Commune,
    Scavenge,
    /// Physical-point forceful actions: open grave, force door, shift rubble.
    Force,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PoolKind {
    Lore,
    Social,
    Mystic,
    Physical,
}

/// Where on the generated maps a clue or feature can live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SiteKind {
    KillSite,
    Npc,
    Grave,
    Church,
    Wilds,
    OutlyingSite,
    Workstation,
}

/// Which church slot a `Church`-site clue anchors to. Authored explicitly:
/// the generator must never infer placement from display prose, because that
/// text is localised and would silently move the world when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChurchSlot {
    Candles,
    Records,
}

impl ChurchSlot {
    /// Map slot id this anchors to.
    pub fn slot_id(self) -> &'static str {
        match self {
            Self::Candles => "church-candles",
            Self::Records => "church-records",
        }
    }
}

// ---------------------------------------------------------------------------
// npcs.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NpcCatalogue {
    pub archetypes: BTreeMap<String, NpcArchetype>,
    pub secrets: BTreeMap<String, SecretTemplate>,
    /// Relationship kinds the generator may draw for NPC links.
    pub relationship_kinds: Vec<RelationshipKind>,
    /// Names for the settled dead: grave markers and revenant identities.
    pub deceased_name_pool: Vec<StringId>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NpcArchetype {
    pub name: StringId,
    pub glyph: char,
    /// Names the generator draws from for this archetype.
    pub name_pool: Vec<StringId>,
    /// Map slot id where this NPC works during the day.
    pub work_slot: String,
    /// Whether this archetype can secretly host the Werewolf.
    pub can_host_villain: bool,
    /// Whether this NPC offers the mystical-favour route.
    pub mystical: bool,
    /// Secret template ids this archetype can carry.
    pub secrets: Vec<String>,
    pub description: StringId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecretTemplate {
    pub name: StringId,
    /// A false secret must be optional and falsifiable via its disproof text.
    pub false_secret: bool,
    pub text: StringId,
    /// Present iff `false_secret`; revealed by the disproving opportunity.
    pub disproof: Option<StringId>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipKind {
    pub id: String,
    pub name: StringId,
    /// Text used when the link is discovered, with {a} and {b} placeholders.
    pub discovered_text: StringId,
}

// ---------------------------------------------------------------------------
// maps/*.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MapTemplate {
    pub name: StringId,
    pub role: MapRole,
    /// Exactly 20 rows of exactly 32 glyphs each.
    pub rows: Vec<String>,
    /// Glyph -> terrain legend for `rows`.
    pub legend: BTreeMap<char, Terrain>,
    /// Named feature slots the generator fills or leaves empty.
    pub slots: Vec<SlotDef>,
    /// Paired exits to the other maps.
    pub exits: Vec<ExitDef>,
    /// Reserved cover pockets (validated against the viability contract).
    pub cover_pockets: Vec<CoverPocket>,
    /// Baseline enemies present from the start of a run.
    #[serde(default)]
    pub initial_enemies: Vec<InitialEnemy>,
    /// Authored variation packs; a run draws a compatible subset per map.
    #[serde(default)]
    pub packs: Vec<VariationPack>,
    pub description: StringId,
}

/// A compatible variation on a template: the same place on a different day.
///
/// A pack may move an anchor, rewrite a little geometry, add ordinary enemy
/// pressure, or merely be a line of incidental fiction. The planner is
/// geometry-blind — it reasons about maps, pools and gates, never tiles — so
/// no pack can make a certified route depend on it; what a pack *can* do is
/// wall something off by accident, which validation and generation both check
/// for by flood-fill.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VariationPack {
    pub id: String,
    /// One line of incidental fiction, logged on first arrival.
    pub label: StringId,
    /// Slot id -> new coordinate on this template.
    #[serde(default)]
    pub slot_moves: BTreeMap<String, Coord>,
    /// Terrain rewrites.
    #[serde(default)]
    pub terrain_patches: Vec<TerrainPatch>,
    /// Extra ordinary enemies, clustered near a slot.
    #[serde(default)]
    pub extra_enemies: Vec<InitialEnemy>,
    /// Packs on the same template that rewrite the same ground and may not
    /// be drawn together.
    #[serde(default)]
    pub conflicts_with: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainPatch {
    pub at: Coord,
    pub to: Terrain,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InitialEnemy {
    /// Enemy id in enemies.toml.
    pub enemy: String,
    /// Slot id the spawn clusters around.
    pub near_slot: String,
    pub count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MapRole {
    Settlement,
    Wilderness,
    OutlyingSite,
}

impl MapRole {
    /// Every role, in the fixed order maps are laid out in a world.
    /// Settlement is always first, and so always `MapId(0)`.
    pub const ORDER: [MapRole; 3] = [
        MapRole::Settlement,
        MapRole::Wilderness,
        MapRole::OutlyingSite,
    ];

    /// Resolve the role a content anchor names. Content says "outlying"
    /// because that is what the place is called; the role is spelled out.
    pub fn from_content(name: &str) -> Option<MapRole> {
        match name {
            "settlement" => Some(MapRole::Settlement),
            "wilderness" => Some(MapRole::Wilderness),
            "outlying" | "outlying-site" => Some(MapRole::OutlyingSite),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MapRole::Settlement => "settlement",
            MapRole::Wilderness => "wilderness",
            MapRole::OutlyingSite => "outlying",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Terrain {
    Floor,
    Wall,
    Tree,
    Water,
    Grave,
    Door,
    /// Requires a Physical point to force open.
    BarredDoor,
    /// Requires a Physical point to shift.
    Rubble,
    Road,
    Grass,
    Altar,
    Workstation,
}

/// Whether actors can stand on this terrain.
///
/// These classifiers live beside the enum so movement semantics are written
/// once: the sim's walkability, the validator's reachability, and the
/// generator's flood fills all compose these rather than re-listing variants.
pub fn is_walkable(terrain: Terrain) -> bool {
    matches!(
        terrain,
        Terrain::Floor | Terrain::Door | Terrain::Road | Terrain::Grass | Terrain::Grave
    )
}

/// Whether this terrain can be cleared with a Physical-point forceful action.
pub fn is_forceable(terrain: Terrain) -> bool {
    matches!(terrain, Terrain::BarredDoor | Terrain::Rubble)
}

/// Whether this terrain blocks line of sight (and pounce lanes).
pub fn is_opaque(terrain: Terrain) -> bool {
    matches!(
        terrain,
        Terrain::Wall | Terrain::Tree | Terrain::Rubble | Terrain::BarredDoor
    )
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlotDef {
    pub id: String,
    pub kind: SiteKind,
    pub at: Coord,
    /// Display name for a landmark placed here. Authored rather than derived
    /// from `id`, so the structural id stays free to change and the prose
    /// stays free to be rewritten or translated.
    pub label: StringId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExitDef {
    /// Map id this exit leads to.
    pub to: String,
    pub at: Coord,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoverPocket {
    /// Tiles forming this pocket; must be opaque terrain in `rows`.
    pub tiles: Vec<Coord>,
}

// ---------------------------------------------------------------------------
// gathers.toml
// ---------------------------------------------------------------------------

/// A placeable gathering/looting opportunity anchored to a map slot.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GatherDef {
    pub name: StringId,
    /// Map template id the anchor slot belongs to.
    pub map: String,
    /// Slot id the opportunity sits on.
    pub slot: String,
    /// Pool the action draws from; omit for a free interaction.
    pub pool: Option<PoolKind>,
    #[serde(default)]
    pub cost: u8,
    /// Item ids granted (duplicates encode quantity).
    pub items: Vec<String>,
    pub discovery: GatherDiscovery,
    pub prompt: StringId,
    pub reveal: StringId,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GatherDiscovery {
    /// Discovered when the anchor tile is first seen.
    Sight,
    /// Only revealed by resolving the named clue template.
    RevealedByClue { clue: String },
    /// Discovered by sight, or revealed early by the named clue template.
    SightOrClue { clue: String },
}

// ---------------------------------------------------------------------------
// grimoire.toml / ui.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrimoireEntry {
    pub id: String,
    pub title: StringId,
    /// Fictional prose; numeric rules stay out of the grimoire by design.
    pub body: StringId,
}

// ---------------------------------------------------------------------------
// openings.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningsFile {
    pub openings: Vec<OpeningDef>,
    pub conditions: Vec<ConditionDef>,
}

/// What the valley is like as the hunter comes into it.
///
/// A run draws exactly one, which is the whole design: most are texture, a few
/// have teeth, and because only one is drawn no run ever carries two things
/// that bite at once.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionDef {
    pub id: String,
    pub axis: ConditionAxis,
    /// Absent means cosmetic.
    #[serde(default)]
    pub effect: Option<ConditionEffect>,
    pub body: Vec<StringId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConditionAxis {
    Season,
    Reception,
    Hour,
    Provenance,
}

impl ConditionAxis {
    /// Every axis, in the order a run's conditions are drawn and read.
    pub const ORDER: [ConditionAxis; 4] = [
        ConditionAxis::Season,
        ConditionAxis::Reception,
        ConditionAxis::Hour,
        ConditionAxis::Provenance,
    ];
}

/// What an effectful condition does.
///
/// Deliberately closed and small. Note what is absent from the banes: nothing
/// touches the final fight. Route certification promises the hunt is winnable,
/// and a bane is not allowed to quietly take that back. Boons are unconstrained
/// in that respect — they can only ever make a certified route easier to walk.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ConditionEffect {
    // -- Banes -------------------------------------------------------------
    /// Consequential Social actions cost one more. The planner must certify
    /// with this applied, since it changes what a route can afford.
    SocialSurcharge,
    /// Percentage points added to the wilderness ambush chance.
    Ambush { percent: u8 },
    /// Tiles taken off the hunter's field of view.
    ShortSight { tiles: u8 },
    /// Extra ordinary enemies away from the settlement.
    Pressure { extra: u8 },

    // -- Boons -------------------------------------------------------------
    /// Percentage points taken off the wilderness ambush chance.
    QuietRoads { percent: u8 },
    /// Tiles added to the hunter's field of view.
    LongSight { tiles: u8 },
    /// One extra of an item in the pack at the start.
    WellSupplied { item: String },
}

impl ConditionEffect {
    /// Whether this makes the run worse. Every run draws exactly one bane and
    /// exactly one boon, from different axes.
    pub fn is_bane(&self) -> bool {
        matches!(
            self,
            ConditionEffect::SocialSurcharge
                | ConditionEffect::Ambush { .. }
                | ConditionEffect::ShortSight { .. }
                | ConditionEffect::Pressure { .. }
        )
    }

    /// Whether route certification has to be run with this applied.
    ///
    /// Only a bane can force this. A boon relaxes what a route can afford, so
    /// a route certified without it stays walkable with it.
    pub fn is_certification_visible(&self) -> bool {
        matches!(self, ConditionEffect::SocialSurcharge)
    }
}

impl ConditionDef {
    pub fn is_cosmetic(&self) -> bool {
        self.effect.is_none()
    }

    pub fn is_bane(&self) -> bool {
        self.effect.as_ref().is_some_and(|e| e.is_bane())
    }

    pub fn is_boon(&self) -> bool {
        self.effect.as_ref().is_some_and(|e| !e.is_bane())
    }
}

/// How a run opens. A generic hook frames the hunt and claims nothing about
/// the case; a keyed one explains the single node the hunter already holds
/// when play begins.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningDef {
    pub id: String,
    /// Where the banked node sits. Absent means a generic hook.
    #[serde(default)]
    pub anchor: Option<OpeningAnchor>,
    /// What the banked node gave. Absent means a generic hook.
    #[serde(default)]
    pub grant: Option<OpeningGrant>,
    /// Prose paragraphs; `{npc}`, `{clue}`, and `{place}` are substituted.
    pub body: Vec<StringId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpeningAnchor {
    /// Anchored on a place: the hunter stopped on the way past.
    Tile,
    /// Anchored on a person: somebody got to the hunter first.
    Npc,
}

/// The kinds of node that may be banked before play. Deliberately narrower
/// than `OpportunityGrant`: a discriminating identity clue is the one that
/// rules alternatives out, and starting with it would leave a single
/// ambiguous sign between the player and the villain's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpeningGrant {
    Items,
    Lead,
    /// A soft identity sign, never a discriminating one.
    Identity,
}

impl OpeningDef {
    /// Whether this opening frames the hunt without explaining a banked node.
    pub fn is_generic(&self) -> bool {
        self.anchor.is_none() && self.grant.is_none()
    }

    pub fn matches(&self, anchor: OpeningAnchor, grant: OpeningGrant) -> bool {
        self.anchor == Some(anchor) && self.grant == Some(grant)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UiText {
    pub splash_title: StringId,
    /// One id per paragraph. Paragraph count is structure, so it stays here
    /// rather than becoming newlines inside a single string-table cell.
    pub splash_intro: Vec<StringId>,
    pub key_bindings: Vec<KeyBinding>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeyBinding {
    pub keys: StringId,
    pub action: StringId,
}

// ---------------------------------------------------------------------------
// machines.toml
// ---------------------------------------------------------------------------

/// An authored optional machine embedded in one template: a visible device
/// with a lever's worth of interaction and an observable payoff. Machines use
/// the ordinary Interact command and the ordinary opportunity affordances;
/// the planner never models them, so a certified route can never lean on one.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachineDef {
    pub name: StringId,
    /// The template this machine is built into.
    pub template: String,
    /// Where the lever or counter sits.
    pub at: Coord,
    pub effect: MachineEffect,
    pub prompt: StringId,
    pub reveal: StringId,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MachineEffect {
    /// Rewrite terrain: a sluice drains a ford, a hidden passage opens.
    Patch { patches: Vec<TerrainPatch> },
    /// Drive every enemy on the map away from the machine.
    Scatter { tiles: u8 },
    /// Raise warding ground centred on the machine.
    Ward { turns: u8, radius: u8 },
}

// ---------------------------------------------------------------------------
// events.toml
// ---------------------------------------------------------------------------

/// One optional mission event. Each generated map receives a small seeded
/// deck of these, filtered by role and scheme; one fires per arrival until
/// the deck runs dry. Every effect is additive — an event may add pressure,
/// supply, or information, never remove access.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    /// Map roles this event can appear on ("settlement", "wilderness",
    /// "outlying").
    pub roles: Vec<String>,
    /// Schemes this event belongs to; empty means any.
    #[serde(default)]
    pub schemes: Vec<String>,
    pub effect: EventEffect,
    /// Logged when the event fires.
    pub body: StringId,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EventEffect {
    /// Pure fiction.
    None,
    /// An enemy arrives nearby.
    Spawn { enemy: String, count: u8 },
    /// Something changes hands.
    Cache { items: Vec<String> },
    /// An undiscovered opportunity on this map announces itself.
    Reveal,
}
