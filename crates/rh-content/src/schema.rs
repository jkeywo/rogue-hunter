//! Serde schema for the authored TOML content files.
//!
//! Every struct uses `deny_unknown_fields` so a typo in a content file fails
//! validation instead of silently vanishing. All gameplay numbers live here
//! rather than as code constants, per the authored-content-catalogue spec.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
    pub name: String,
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
    pub name: String,
    pub stamina_cost: u8,
    pub effect: ManoeuvreEffect,
    pub description: String,
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
    pub name: String,
    pub physical_cost: u8,
    pub effect: SignatureEffect,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SignatureEffect {
    /// Place a snare on an adjacent tile; first enemy entering is held.
    SetSnare,
    /// Double-damage melee attack against an immobile or wounded enemy.
    KillingBlow,
}

// ---------------------------------------------------------------------------
// enemies.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnemyDef {
    pub name: String,
    pub glyph: char,
    pub health: u16,
    pub melee_damage: u16,
    /// Attack hit chance in whole percent.
    pub hit_percent: u8,
    pub behaviour: EnemyBehaviour,
    /// Optional ranged attack (bandits).
    pub ranged: Option<EnemyRanged>,
    pub description: String,
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
    pub name: String,
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
    /// Item id that acts as this villain's decisive weakness counter.
    pub weakness_item: String,
    /// Whether the church consecration rite affects this villain.
    pub affected_by_consecration: bool,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TierBehaviour {
    pub id: String,
    pub name: String,
    pub effect: TierEffect,
    /// Event-log telegraph shown when the tier activates.
    pub telegraph: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TierEffect {
    PounceCooldown { turns: u8 },
    DashCooldown { turns: u8 },
    BonusMeleeDamage { amount: u16 },
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
    pub telegraph: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegenerationDef {
    /// Health regained per encounter turn until stopped by the weakness item.
    pub per_turn: u16,
    pub telegraph: String,
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
    pub vulnerable_telegraph: String,
    pub dash_telegraph: String,
    pub guarded_telegraph: String,
}

// ---------------------------------------------------------------------------
// origins.toml / schemes.toml
// ---------------------------------------------------------------------------

/// An origin changes the villain's signs (which clue templates apply) and
/// which weakness-ingredient sources the generator seeds.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OriginDef {
    pub name: String,
    /// Clue-site kinds this origin emphasises when placing identity clues.
    pub sign_sites: Vec<SiteKind>,
    pub description: String,
}

/// A scheme controls the timed events and which minion family serves the villain.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemeDef {
    pub name: String,
    /// Enemy id of the minion family this scheme fields.
    pub minion_enemy: String,
    pub minor_event: SchemeEvent,
    pub major_event: SchemeEvent,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemeEvent {
    pub name: String,
    /// Event-log text when the event fires.
    pub text: String,
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
    pub name: String,
    pub glyph: char,
    pub kind: ItemKind,
    pub description: String,
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
    pub name: String,
    /// Item ids consumed (duplicates allowed for quantities).
    pub inputs: Vec<String>,
    /// Item id produced.
    pub output: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// clues.toml
// ---------------------------------------------------------------------------

/// Template for a generated clue opportunity.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClueTemplate {
    pub name: String,
    /// Villain id this clue can point at, or "any".
    pub archetype: String,
    /// Origin ids this clue fits; empty means any origin.
    #[serde(default)]
    pub origins: Vec<String>,
    pub category: ClueCategory,
    pub action: OpportunityAction,
    pub pool: PoolKind,
    /// Site kind where the generator may place this clue.
    pub site: SiteKind,
    /// 0 = obvious .. 3 = niche. Fallback routes prefer low totals.
    pub obscurity: u8,
    /// Opportunity text shown before the clue is taken.
    pub prompt: String,
    /// Event-log / journal text once revealed.
    pub reveal: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClueCategory {
    /// Corroborating identity evidence; two distinct ones uncover the villain.
    Identity,
    /// Reveals where the villain rests or who hosts it.
    Location,
    /// Reveals a specific weakness preparation (e.g. the candles are silver).
    Weakness,
    /// Grants crafting ingredients.
    IngredientSource,
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NpcArchetype {
    pub name: String,
    pub glyph: char,
    /// Names the generator draws from for this archetype.
    pub name_pool: Vec<String>,
    /// Map slot id where this NPC works during the day.
    pub work_slot: String,
    /// Whether this archetype can secretly host the Werewolf.
    pub can_host_villain: bool,
    /// Whether this NPC offers the mystical-favour route.
    pub mystical: bool,
    /// Secret template ids this archetype can carry.
    pub secrets: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecretTemplate {
    pub name: String,
    /// A false secret must be optional and falsifiable via its disproof text.
    pub false_secret: bool,
    pub text: String,
    /// Present iff `false_secret`; revealed by the disproving opportunity.
    pub disproof: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipKind {
    pub id: String,
    pub name: String,
    /// Text used when the link is discovered, with {a} and {b} placeholders.
    pub discovered_text: String,
}

// ---------------------------------------------------------------------------
// maps/*.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MapTemplate {
    pub name: String,
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
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MapRole {
    Settlement,
    Wilderness,
    OutlyingSite,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlotDef {
    pub id: String,
    pub kind: SiteKind,
    pub at: Coord,
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
// grimoire.toml / ui.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrimoireEntry {
    pub id: String,
    pub title: String,
    /// Fictional prose; numeric rules stay out of the grimoire by design.
    pub body: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UiText {
    pub splash_title: String,
    pub splash_intro: Vec<String>,
    pub key_bindings: Vec<KeyBinding>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeyBinding {
    pub keys: String,
    pub action: String,
}
