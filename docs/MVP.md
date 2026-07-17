# Rogue Hunter MVP

## Product statement

Rogue Hunter is an early-modern ASCII monster-hunting roguelike. Each seeded run generates a small, solvable folk-horror mystery across a settlement, wilderness, and outlying site. The hunter investigates, prepares counters, and survives a final pursuit by the villain.

## MVP success condition

The same displayed seed and semantic command replay must produce an identical run in the terminal and the GitHub Pages WebAssembly client. Generation must construct and validate two feasible, costed paths to a viable final hunt before materialising the world.

## In scope

- Rust workspace with an authoritative headless simulation, terminal client, and WASM web client.
- Native PC presentation uses Bevy + Ratatui. The web presentation is a separate Canvas/HTML WASM view; both consume the same simulation state and semantic commands.
- Both clients support mouse input: hover inspects visible tiles, actors, opportunities, and UI; clicks select targets and issue the same semantic actions as keyboard controls.
- Deterministic seeded generation from authored data only; no runtime AI generation.
- Monsters, origins, schemes, items, clues, NPCs, and map templates live in human-editable declarative content files, validated by schema and generator checks in CI.
- Three generated 32×20 tactical maps: settlement, wilderness, and outlying site. They form a triangle; the wilderness-to-outlying route has a seed-defined ambush chance.
- Inter-map travel uses paired generated exit tiles. Reaching one is local movement; using it advances the global clock and places the hunter at the paired destination exit.
- The three-region travel map is visible from the start. Each tactical map uses local fog of war; opportunities appear only when discovered or learned about.
- Six travel turns. Travel, fleeing, death, and authored costly actions advance the global clock; all local movement and interaction remain turn-based tactical actions but do not advance that global clock.
- The generated villain scheme creates a minor event on turn 2 and a major event on turn 4. Each raises a stackable villain threat tier—+3 health plus one enhanced behaviour—making a prepared early hunt preferable; turn 6 begins the final hunt.
- Lore, Social, and Mystic investigation pools. The MVP hunter has caps of Lore 2, Social 2, Mystic 0; travel restores one point to every pool up to its cap.
- Every generated opportunity is visible when discovered. If its required pool is empty, the UI explains why it cannot currently be taken rather than hiding the action.
- Physical is a fourth, scarce point pool for powerful signature abilities and strenuous shortcuts; it restores one point on every global-clock advance, and the fixed hunter has Physical 2. Stamina is separate: the fixed hunter has Stamina 4, it restores each encounter turn, and it fuels special moves.
- Physical also unlocks visible forceful opportunities, such as opening graves, forcing barred doors, and shifting rubble.
- Generic Stamina manoeuvres are shared by future hunters; hunter-specific Physical-point signatures express the fixed MVP hunter's identity. MVP manoeuvres are Aim (2 Stamina; next ranged attack always hits), Power Attack (2 Stamina; next melee attack deals ×1.5 damage), and Sprint (1 Stamina; move two tiles in one action).
- The fixed Huntress has two Physical-point signatures: Set Snare spends 1 Physical point to trap the first enemy entering an adjacent tile for three encounter turns. Trapped enemies cannot move, pounce, or dash; they retain adjacent attacks at −25 percentage points to hit. Killing Blow spends 1 Physical point for double melee damage against an immobile enemy or one at 50% health or less.
- A temporary over-cap Mystic point from an optional mystical-NPC favour route.
- All eight Werewolf/Revenant × two-origin × two-scheme combinations are supported and solver-validated. Origins change signs and weaknesses; schemes control events and minions.
- Ordinary enemy families are wolves, bandits, and restless dead.
- Ordinary enemies have an initial 15% low-chance, seed-determined drop rate for ammunition, ingredients, coin, or clues. Validated hunt-ready routes never depend on these lucky drops.
- Three generated NPCs with dispositions, secrets, and at least two relationship links. False secrets must be falsifiable and optional.
- NPCs appear on a relationship map once met; their links remain hidden until discovered through Social actions or spying.
- Villagers and neutral NPCs take routine local turns—moving, working, and talking—so relationships can be observed and Social opportunities are spatially alive.
- Ordinary conversation is free; consequential Social actions—gossip, persuasion, spying, exposing a secret, or requesting a favour—spend a Social point.
- Every tactical map is always turn-based. The hunter can move, melee, make ranged attacks, use an item, and wait; active enemies act after every hunter action. These local turns do not advance the global clock.
- Combat uses a small, visible integer scale: the fixed Huntress has 12 health, ordinary enemies roughly 3–6, and villains roughly 18–24. Values remain authored-data tuning rather than code constants.
- The fixed hunter carries a melee weapon and three ordinary flintlock shots. Ammunition can be found or traded; silver scavenged from church candles or mined in the wilderness crafts into a silver bullet against a Werewolf. It deals massive damage and stops regeneration; Aim makes the next ranged shot certain to hit.
- The Werewolf has a line-of-sight pounce with a three-turn cooldown. Obstacles that break its lane are a core tactical defence; hunter special moves help exploit this and other monster patterns.
- Each generated map reserves limited, deliberate cover pockets—such as trees, carts, walls, or gravestones—as part of its combat-viability contract.
- Crafting from gathered ingredients at a workstation. Weakness items improve damage and counter a specific villain behaviour, but early hunts remain possible.
- A craftable wound draught restores 4 health but consumes the hunter's encounter action.
- An optional one-turn church-consecration rite creates a warded settlement combat space. A Revenant is normally vulnerable only once every five encounter turns and takes no direct damage otherwise; on consecrated ground it takes ongoing damage and remains vulnerable. Its five-turn dash cadence lets it retreat before vulnerability or close on a distant hunter. The rite is useless against a Werewolf.
- A consumable binding charm is the alternate Revenant counter: used adjacent, it forces five consecutive encounter turns of vulnerability but does not stop its dash.
- Two corroborating identity clues formally uncover an early hunt: a Revenant lies dormant in a generated grave (potentially near the church), while a Werewolf is secretly one of the generated NPCs. The hunter may still gamble by attacking a villager without proof or spending 1 Physical point opening a random grave.
- Attacking or killing an innocent villager creates severe social fallout: settlement hostility and loss of that NPC's resources/information, but no automatic global-clock cost. The generated alternate route keeps the run viable.
- On death, retain information and inventory, respawn at the settlement, and lose one global turn. At turn six the villain appears somewhere on the current map and pursues the hunter. Voluntary time-costing actions that would end the mission—including map travel—are blocked.
- Replay saves using a base seed plus semantic commands. A single deterministic simulation PRNG drives generation and runtime random events.
- Replays use compact share codes—seed plus semantic command log—for copy/paste between terminal and browser, bug reports, and shared runs.
- Active runs persist as replay saves: local files for the native client and browser local storage for the web client.
- A completed or failed run ends with a case report revealing the villain, origin, scheme, hidden clues, certified routes, and replay code.
- An opening splash screen explains the premise and current key bindings before a run begins, then offers New Run, Enter Seed, and Paste Replay Code.
- GitHub Pages deployment from the default branch after checks pass.
- Every pull request runs PASM validation, replay checks, the bounded local generator corpus, and native/WASM builds. Only successful default-branch builds deploy GitHub Pages.
- A fully unlocked in-game grimoire documents every MVP monster, origin, scheme, pattern, weakness, and term in fiction rather than numerical rules. Exact timing is made observable in play through clear messages, such as a Revenant's vulnerability announcement.
- A persistent event log records those telegraphs alongside combat rolls, regeneration, pounces, clue discoveries, and clock events in both clients and replays.

## Generator contract

The generator first builds a directed, costed clue graph, then materialises it into maps, opportunities, NPCs, items, events, and encounters. It must:

1. Track the selected hunter's exact investigation pools, inventory, travel turns, favours, and combat state.
2. Find an early, hunt-ready route by turn 3. It may include obscure or niche actions.
3. Penalise the early route's used/niche nodes and find a more obvious, hunt-ready fallback route by turn 5.
4. Keep both routes below separately tunable travel and weighted-effort bounds, while requiring the planner's combat-viability threshold.
5. Allow a mystical-NPC boon on no more than one required route.
6. Provide at least two reachable routes to an effective weakness and villain location.
7. Rate each certified final hunt at least 75% viable using the combat heuristic. Bad-roll losses remain possible in play.
8. Run a final validation over the assembled world before returning it.

Generator stress validation must remain below 30 seconds for the local corpus and below five minutes for the CI corpus; corpus size is tuned to those budgets while covering every villain combination.

## Explicit deferrals

- Character-choice generation and multiple hunters.
- Meta-progression, levelling, and between-run unlocks.
- Arbitrary biome/map-role combinations.
- Runtime generative AI and graphical tiles.

## Implementation order

1. Headless deterministic simulation, graph-first generator, solvability planner, and replay harness.
2. Terminal ASCII client over the same command/state interface.
3. WASM ASCII client and GitHub Pages deployment over that unchanged interface.

The headless toolchain includes a developer-only generation inspector showing the seed, clue graph, certified routes, node costs, and candidate rejection reasons.
