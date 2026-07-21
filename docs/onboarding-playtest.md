# First-run onboarding: audit and playtest protocol

Onboarding is a validation task before a building task. This document is the
deliverable of the validation half: a cold-reader audit of what a first-time
player must do and where each thing is taught, the candidate stalls that fall
out of it, the fixes already made blind, and a protocol for the playtest that
answers only what actually stalls. **The building half waits for that playtest**
— per the milestone, a scripted tutorial or new guide prose written from the
inside, by someone who already knows the answer, would compound the very gap
this is meant to close.

## The cold-reader audit

Every step between the splash and a first correct naming, and where the
knowledge to take it comes from: the splash bindings table, one of the eight
`content/guide.toml` entries, one of the state-triggered `check_hints` lines, or
**nowhere**.

| Step the player must take | Taught by |
|---|---|
| Start a run, pick a hunter | Splash options; hunter-select shows each hunter's pools |
| Read the opening, know the six-day clock | Opening prose; guide *The Clock*; `day-passed` hint |
| Move; that a menu is navigable | Splash bindings (keys + control scheme) |
| Interact with a lead to investigate | Splash binding `e`; action panel row |
| That a lead costs a pool point | **was nowhere** → now the `unaffordable` hint |
| That pools restore on travel | **was nowhere** → now the `unaffordable` hint |
| Gather across three maps; travel spends a day | Guide *The Hunt Itself*; travel prose |
| That two proofs, one discriminating, name the quarry | Guide *Naming the Thing* |
| That the proofs now suffice — the moment to name | **was nowhere** → now the `can-name` hint |
| Name the villain (uncover) | Action panel row when eligible |
| Why a greyed-out action is greyed | **was nowhere** → now disabled-implies-note (Stage D) |
| What a glyph on the map is | **was nowhere** → now the map key (Stage D) |
| Where to read all of this again | Guide on `i`; **now also `?`**, the universal help key |

## Candidate stalls, and what was done

Five "nowhere" rows above were the candidate stalls. Four are closed blind,
because each is a fact the game can state without a script drifting from the
rules:

1. **A lead you cannot afford** — a first-timer hits a pool wall without knowing
   it is one. New `ui.hint.first.unaffordable`, fired the first time a
   discovered lead costs more of a pool than the hunter holds.
2. **The proofs now agree** — the pivot of the whole case, previously silent.
   New `ui.hint.first.can-name`, fired the first time corroboration is met and
   the villain is not yet named.
3. **A greyed-out action** — closed in Stage D: every disabled action now
   carries the reason it is disabled, enforced where actions are built.
4. **An unfamiliar glyph** — closed in Stage D: the map has a written key.
5. **Finding the how-to again** — the guide answered only to `i`; it now also
   answers to `?`, the key a lost player reaches for by reflex.

None of these is a tutorial. Each is a reference or a one-line reaction to game
state, so none can become a second, drifting copy of the rules.

## What waits for the playtest

- Whether a first player finds `Interact` at all, or needs the action panel to
  say more plainly what it is for.
- Whether the opening's six-day clock lands, or reads as flavour.
- Whether the guide is opened unprompted, or needs a nudge on the first run.
- Whether any splash prose should be reordered or cut.

These are comprehension questions. They cannot be answered from the inside; the
audit above narrows where to look so the playtest is a targeted spot-check, not
an open-ended watch.

## Observation protocol

Watch one first-time player, unaided, from a cold start. Say nothing. Note, in
order and with the turn it happened:

1. **Splash → run.** Do they pick a hunter deliberately, or click through? Do
   they read the control scheme row?
2. **First move.** How long before the hunter moves? Which keys do they try?
3. **First interaction.** Do they find `Interact` on a lead within the first
   day? If not, what did they try instead?
4. **First pool wall.** When a lead is unaffordable, does the hint land — do they
   travel to restore, or stall? Note whether they read the hint at all.
5. **First naming.** When the proofs agree, do they name the quarry, or keep
   investigating past the point they could have stopped? Note whether the
   `can-name` hint was on screen.
6. **The guide.** Do they ever open it (`i` or `?`)? At what point, and did
   something drive them to it?
7. **Any greyed action.** Do they read the reason, or retry the key?

For each, record only: *did they stall, and on what*. The second, small building
pass answers exactly those stalls and nothing more.
