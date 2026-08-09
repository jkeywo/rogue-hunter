@RTK.md

# Rogue Hunter — Agent Guide

An early-modern ASCII monster-hunting roguelike. Six in-game days to identify
and kill either a Werewolf hiding among the villagers or a Revenant in a
dormant grave, across three tactical maps (settlement, wilderness, outlying)
under a travel clock. Every villain × origin × scheme combination is
solver-validated before it ships.

## Tech stack

| Layer | Technology |
|---|---|
| Content | `crates/rh-content` — authored TOML catalogue, `include_str!`-embedded, schema + cross-reference validation |
| Simulation | `crates/rh-core` — deterministic headless sim: state, semantic commands, rules, AI, FOV |
| Generation | `crates/rh-gen` — graph-first mystery generator, solvability planner, materialiser |
| Replay | `crates/rh-replay` — share codes, command logs, replay execution, autoplayer, corpus |
| Client core | `crates/rh-client-core` — UI-agnostic session state machine, viewmodel, input intents |
| Clients | `crates/rh-terminal` (Bevy as a frame pump + Ratatui), `crates/rh-web` (wasm-bindgen + Canvas/DOM, **no Bevy**) |
| Dev CLI | `crates/rh-cli` (bin `rh`) — generate, autoplay, corpus, replay checks |
| Shared crates | vellum-digest, vellum-replay, vellum-rng, vellum-strings — pinned by rev |
| Architecture model | PASM — YAML spec under `pasm/spec/`, tool pinned from vellum |
| CI | fleet-ci caller (`.github/workflows/ci.yml`) → pasm gates + scenario replays, clippy `-D warnings`, tests, bounded corpus, wasm-pack build, Pages deploy |

## Determinism is the whole architecture

**A save is a seed plus a command log.** Nothing is snapshotted; loading
replays. That makes the RNG byte stream and every serialised shape reachable
from sim state *part of the save format*:

- This repo is the fleet's **sacred tier** (vellum `docs/handbook/determinism.md`).
  A shared-crate change that moves a fingerprint fails vellum's consumer smoke
  matrix before a bump PR here exists.
- `REPLAY_VERSION` gates share codes: a rules change that invalidates old logs
  bumps it, so an old code is *refused* rather than misread. Its doc comment
  records why each version moved — keep that history.
- Fixtures (`*golden*`, `*fixture*`, `*trace*`) are load-bearing artifacts.
  Never re-bless one to make a build green; a moved fixture is a finding.
  The bless switches (`RH_BLESS_RNG_TRACE`, `RH_BLESS_GOLDEN`) exist for
  deliberate, reviewed format changes only.
- Content is `include_str!`-embedded so native, wasm, and CI ship
  byte-identical data. `content_fingerprint()` skips `\r` — a CRLF checkout
  must not produce a different fingerprint from a LF one, and once did.

## Text — never write a string literal

Every player-facing string lives in `content/strings.csv`, embedded
separately and **excluded from the content fingerprint**, so a copy edit or a
translation never invalidates a share code.

- The invariant: anything the RNG indexes, or that generation branches on,
  stays in TOML. `strings.csv` holds only what is rendered and never read.
- Agent-written copy is wrapped in `[square brackets]`; a test enforces it
  until a human replaces the line.
- **Never branch on text.** Give presentation a typed value and let the words
  be a lookup.

## PASM — keep it up to date

1. Model first, then build — spec entities before Rust for a new system.
2. Record decisions in `pasm/spec/core/decisions.yaml`, including negative
   results (`dominance-pruning-did-not-help` is a real entry and earns its
   place).
3. `uv run pasm validate pasm/spec` after any model change.
4. `uv run pasm scan pasm/spec --json` gates CI — keep implementation
   mappings current.
5. Scenario replays under `tests/replays/*.yaml` run through `pasm scenario`
   in CI; they are spec artifacts, not test fixtures.

## Common commands

```bash
# CI gates — run all of these before calling work done
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run --release -p rh-cli -- corpus --count 256 --budget-seconds 300
uv run pasm validate pasm/spec

# Play it
cargo run --release -p rh-terminal        # or build_and_run.bat

# Web client (deliberately not Bevy: wasm-bindgen + a hand-written shell)
wasm-pack build crates/rh-web --target web --release --out-dir ../../web/pkg
python -m http.server 8571 --directory web
```

## Vellum — the shared foundation

This repo pins vellum by rev in `Cargo.toml`, `pyproject.toml`, and the
`uses:` line of `.github/workflows/ci.yml`. A vellum bump PR aligns every pin
and touches nothing else — a diff reaching further means the engine changed
behaviour. For local work against a checkout use a **gitignored**
`.cargo/config.toml` `[patch]`; a committed override would build CI against
whatever happened to be on disk.

## AI-origin decisions

A decision you (an agent) make while working is marked in the spec:
`origin: ai` on the entity you originated, or a literal `[ai] ` prefix on the
rationale bullet you wrote. Unmarked decisions are the human's. AI-origin
items may be revised without asking when evidence warrants — say so in the
commit. Never alter an unmarked decision without asking, and never remove a
marker: ratification is the human deleting it after reviewing

```bash
uv run pasm review pasm/spec
```
