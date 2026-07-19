# Rogue Hunter

Play now: **https://jkeywo.github.io/rogue-hunter/**

Rogue Hunter is an early-modern ASCII monster-hunting roguelike. Each seeded run generates a small, solvable folk-horror mystery across a settlement, a wilderness, and an outlying site. Investigate, prepare your counters, and survive a final pursuit by the villain — a Werewolf hiding among the villagers, or a Revenant sleeping in a dormant grave.

You have six days. Read the signs the thing cannot help but leave, prove its name twice over, prepare its death, and take the hunt to it — or let the sixth night come, and it will take the hunt to you.

## Playing

- **Browser**: open the link above. No install.
- **Native terminal**: `build_and_run.bat` (Windows), or `cargo run --release -p rh-terminal` on any platform with a working terminal.

Both clients read the same authored content and the same deterministic simulation, so a run started in one plays out identically in the other — a run's *seed + command log* is its entire save state, encoded as a compact `RH1-...` share code you can copy, paste, and replay.

## Design

The generator is graph-first: it builds a directed, costed clue graph before it ever materialises a map, then a solver certifies two independent routes through that graph — an early, possibly-obscure hunt-ready route by day 3, and a more obvious fallback by day 5 — before the run is allowed to start. Both routes are checked against a combat-viability heuristic, so every generated mystery is provably solvable without lucky drops before the player ever sees it. See [`docs/MVP.md`](docs/MVP.md) for the full product spec and [`pasm/spec/core`](pasm/spec/core) for the recorded design and technical decisions behind the implementation.

## Repository layout

```
content/          Authored TOML game data (monsters, items, clues, NPCs, maps) — no gameplay numbers live in code
content/strings.csv  Every player-facing string, by id, with context and English — see "Writing and translating"
crates/
  rh-content/      Content schema, loading, and validation
  rh-core/         Deterministic headless simulation (state, commands, combat, AI)
  rh-gen/          Graph-first mystery generator and route-certifying planner
  rh-replay/       Share codes, replay execution, autoplayer
  rh-client-core/  Shared session/viewmodel consumed by both clients
  rh-cli/          Headless `rh` dev tool: generation inspector, replay checks, corpus runs
  rh-terminal/     Native client (Bevy + Ratatui)
  rh-web/          Browser client (WebAssembly + Canvas/HTML)
web/               Static shell + built WASM bundle for the browser client
pasm/              PASM specification and validation tooling for this project's design/architecture
docs/              Product spec (MVP.md)
```

## Development

Requires Rust 1.95 (pinned via `rust-toolchain.toml`) and Python 3.11+ for the PASM tooling.

```sh
cargo test --workspace                  # content schema, sim, generator corpus, golden replays
cargo run -p rh-cli -- generate --seed 1  # inspect a generated world's clue graph and certified routes
cargo run -p rh-cli -- autoplay --seed 1  # watch the deterministic bot play a full run
cargo run -p rh-terminal                  # native client

pip install -e .
pasm validate pasm/spec                 # validate the design spec
```

To build the browser client locally:

```sh
wasm-pack build crates/rh-web --target web --release --out-dir ../../web/pkg
python -m http.server 8571 --directory web   # then open http://localhost:8571
```

## Writing and translating

Every string a player reads lives in `content/strings.csv`, one row per string:

| column | what it is |
| --- | --- |
| `id` | stable lookup key. `enemies.wolf.name` and friends come from the content files; `ui.*`, `log.*` and `gen.*` are named by code |
| `context` | where the line appears and how it is used, for whoever writes or translates it |
| `english` | the text |

**Every line is currently wrapped in `[square brackets]`.** All of it was
written by an agent, and the brackets say so. To write real copy, replace a
row's English and delete its brackets. Nested brackets mean a template and the
thing it names are both still placeholder, and they clear independently.

The bracket gate is a test, not a load-time error, so partially-written copy
never breaks the build. Once enough real copy lands, narrow
`every_string_is_bracketed_placeholder_copy` in `crates/rh-content/tests/catalogue.rs`
to the rows still awaiting a writer.

Editing text needs a rebuild — the table is embedded at compile time so native,
WASM and CI ship identical content. It is deliberately excluded from
`content_fingerprint`, so rewriting or translating a line leaves existing share
codes valid. That only holds while nothing reads the text back, which is the
rule to keep:

> Anything the RNG indexes, or that generation or the simulation branches on,
> stays in TOML. `strings.csv` holds only strings that are rendered and never
> read.

NPC name pools show how that plays out: the pools stay in `npcs.toml` because
pool length and order are a generation input the RNG indexes, but they hold
ids, so the names themselves are translatable. Which villager is drawn is
generation; what they are called is text. Validation refuses content pointing
at an id that does not resolve, and tests check the reverse: no row goes
unreferenced.

To re-run the extraction after adding prose fields to the schema, add them to
the manifest in `pasm/tools/extract_strings.py` and run it. It refuses to run
while any string in the content files is unclassified, so new prose cannot be
missed by omission.

## CI and deployment

Every push and pull request runs PASM validation, `rustfmt`/`clippy`, the full test suite, a bounded generator corpus, and native + WASM builds. A successful build on `main` deploys the browser client straight to GitHub Pages (see [`.github/workflows/ci.yml`](.github/workflows/ci.yml)) — no separate `gh-pages` branch to manage.

## License

MIT — see [LICENSE](LICENSE).
