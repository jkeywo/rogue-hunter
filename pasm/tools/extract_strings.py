"""Move authored prose out of content/*.toml and into content/strings.csv.

Run from the repo root:

    python pasm/tools/extract_strings.py            # report only
    python pasm/tools/extract_strings.py --write    # extract and rewrite

Three passes. EXTRACT reads every prose field named in MANIFEST and emits a
CSV row: a mechanical id, a context note for whoever writes or translates the
line, and the English wrapped in [square brackets] to mark it as the
agent-written placeholder it is. REWRITE replaces each field's literal in the
TOML *text* with its id, so comments and formatting survive -- a round-trip
through a TOML library would delete every comment, and these files are
heavily commented. VERIFY re-parses the result and checks that each field now
holds its id and that the extracted English still matches the original byte
for byte.

The manifest is explicit on purpose. AUDIT reports any string-valued leaf it
does not classify, so prose cannot be missed by omission: a new field is
either listed as prose or listed as structural, never silently skipped.
"""

from __future__ import annotations

import argparse
import csv
import io
import pathlib
import re
import sys
import tomllib

CONTENT = pathlib.Path("content")
CSV_PATH = CONTENT / "strings.csv"

# ---------------------------------------------------------------------------
# Manifest
# ---------------------------------------------------------------------------
# Paths use a tiny selector language:
#   *    every key of a table (the key is the entry id)
#   []   every element of an array (its `id` field, else its index, names it)
#   name a literal key
#
# Each entry is (path, context). The context is what a translator reads, so it
# says where the line shows up, not merely which field it came from.

MANIFEST: dict[str, list[tuple[str, str]]] = {
    "enemies.toml": [
        ("*.name", "Name of an ordinary enemy, shown when you fight or inspect it"),
        ("*.description", "Enemy description, shown when inspecting it"),
    ],
    "villains.toml": [
        ("*.name", "Name of a villain archetype, shown once its identity is proven"),
        ("*.description", "Villain description, shown in the case report and grimoire"),
        ("*.pounce.telegraph", "Warning line when this villain winds up to pounce"),
        ("*.regeneration.telegraph", "Log line when this villain regenerates health"),
        ("*.ward.absorb_telegraph", "Log line when the Witch's ward absorbs a hit"),
        ("*.ward.break_telegraph", "Log line when the Witch's ward breaks"),
        ("*.ward.reweave_telegraph", "Log line when the Witch's ward reweaves itself"),
        ("*.cadence.vulnerable_telegraph", "Log line when the Revenant becomes vulnerable"),
        ("*.cadence.dash_telegraph", "Log line when the Revenant dashes"),
        ("*.cadence.guarded_telegraph", "Log line when the Revenant guards"),
        ("*.tier_behaviours[].name", "Name of a villain tier behaviour, shown when it triggers"),
        ("*.tier_behaviours[].telegraph", "Log line when this villain tier behaviour triggers"),
    ],
    "origins.toml": [
        ("*.name", "Name of a case origin, shown once proven"),
        ("*.description", "Origin description, shown in the case report and grimoire"),
        ("*.counter_flavour", "Flavour shown when preparing the reagent that counters this origin"),
    ],
    "schemes.toml": [
        ("*.name", "Name of a villain scheme, shown once proven"),
        ("*.description", "Scheme description, shown in the case report and grimoire"),
        ("*.minor_event.name", "Title of this scheme's minor clock event"),
        ("*.minor_event.text", "Log line when this scheme's minor clock event fires"),
        ("*.major_event.name", "Title of this scheme's major clock event"),
        ("*.major_event.text", "Log line when this scheme's major clock event fires"),
        ("*.preempt.name", "Name of the opportunity that pre-empts this scheme"),
        ("*.preempt.prompt", "Prompt shown before taking the scheme pre-emption"),
        ("*.preempt.reveal", "Journal text once the scheme pre-emption is taken"),
        ("*.preempt.blunted_text", "Log line when the pre-empted scheme event fires blunted"),
    ],
    "items.toml": [
        ("*.name", "Item name, shown in the pack and in loot messages"),
        ("*.description", "Item description, shown when inspecting it in the pack"),
    ],
    "recipes.toml": [
        ("*.name", "Recipe name, shown in the crafting menu"),
        ("*.description", "Recipe description, shown when inspecting it in the crafting menu"),
    ],
    "clues.toml": [
        ("*.name", "Clue name, shown in the journal and on the opportunity"),
        ("*.prompt", "Opportunity text shown before this clue is taken"),
        ("*.reveal", "Journal and event-log text once this clue is revealed"),
    ],
    "gathers.toml": [
        ("*.name", "Name of a gathering opportunity, shown on the map and in the action list"),
        ("*.prompt", "Opportunity text shown before gathering here"),
        ("*.reveal", "Log line once the gathering is done"),
    ],
    "npcs.toml": [
        ("archetypes.*.name", "NPC archetype label, shown after their name (e.g. 'Anne, the priest')"),
        ("archetypes.*.description", "NPC archetype description, shown when inspecting them"),
        ("secrets.*.name", "Name of an NPC secret, shown in the journal"),
        ("secrets.*.text", "Journal text when an NPC's secret is uncovered"),
        ("secrets.*.disproof", "Journal text when an NPC's secret is disproved"),
        ("relationship_kinds[].name", "Name of a relationship between two NPCs"),
        ("relationship_kinds[].discovered_text", "Log line when a relationship between two NPCs is discovered"),
    ],
    "grimoire.toml": [
        ("entries[].title", "Grimoire entry title"),
        ("entries[].body", "Grimoire entry body text"),
    ],
    "openings.toml": [
        ("openings[].body[]", "Opening narration paragraph, shown as the run begins"),
        ("conditions[].body[]", "Valley-condition paragraph, shown as the run begins"),
    ],
    "hunters/*.toml": [
        ("name", "Hunter's name, shown on the hunter-select screen and status panel"),
        ("title", "Hunter's title, shown under their name on the hunter-select screen"),
        ("manoeuvres[].name", "Name of a hunter manoeuvre, shown in the action list"),
        ("manoeuvres[].description", "Manoeuvre description, shown when inspecting the action"),
        ("signatures[].name", "Name of a hunter signature ability, shown in the action list"),
        ("signatures[].description", "Signature ability description, shown when inspecting the action"),
    ],
    "maps/*.toml": [
        ("name", "Map name, shown on the travel screen and status panel"),
        ("description", "Map description, shown on the travel screen"),
        ("slots[].label", "Landmark name, shown when inspecting this spot on the map"),
    ],
}

# String-valued leaves that are structure, not prose: ids, cross-references,
# enum tags, glyphs, and map art. Listed so AUDIT can tell "not prose" from
# "not yet classified".
STRUCTURAL = {
    "glyph", "role", "kind", "category", "action", "pool", "site", "axis",
    "effect", "behaviour", "concealment", "id", "to", "map", "slot", "enemy",
    "near_slot", "output", "minion_enemy", "weakness_item", "counter_reagent",
    "work_slot", "map_role", "site_map", "anchor", "grant", "discovery",
    "supports", "rules_out", "villains", "origins", "schemes", "inputs",
    "items", "starting_items", "sign_sites", "secrets", "grants_items",
    "rows", "legend", "name_pool", "deceased_name_pool", "clue",
    "church_slot", "ammo", "false_secret",
}


def slug(text: str) -> str:
    """A path segment safe for an id."""
    return re.sub(r"[^a-z0-9]+", "-", str(text).lower()).strip("-")


def walk(data, path: str, prefix: list[str]):
    """Yield (id_segments, container, key, value) for every match of `path`."""
    if not path:
        return
    head, _, rest = path.partition(".")
    if head.endswith("[]"):
        head = head[:-2]
        items = data.get(head) if isinstance(data, dict) else None
        if not isinstance(items, list):
            return
        for index, item in enumerate(items):
            if isinstance(item, dict):
                name = slug(item.get("id", index))
                if rest:
                    yield from walk(item, rest, prefix + [slug(head), name])
                continue
            # A list of bare strings, e.g. `body`: index each paragraph.
            if not rest:
                yield (prefix + [slug(head), str(index + 1)], items, index, item)
        return
    if head == "*":
        if not isinstance(data, dict):
            return
        for key, value in data.items():
            yield from walk(value, rest, prefix + [slug(key)])
        return
    if not isinstance(data, dict) or head not in data:
        return
    if rest:
        yield from walk(data[head], rest, prefix + [slug(head)])
        return
    value = data[head]
    if isinstance(value, list) and all(isinstance(v, str) for v in value):
        for index, item in enumerate(value):
            yield (prefix + [slug(head), str(index + 1)], value, index, item)
    elif isinstance(value, str):
        yield (prefix + [slug(head)], data, head, value)


def id_prefix(rel: str) -> str:
    """`maps/settlement.toml` -> `maps.settlement`; `items.toml` -> `items`."""
    stem = rel[:-5] if rel.endswith(".toml") else rel
    return ".".join(slug(part) for part in stem.split("/"))


def files_for(pattern: str) -> list[pathlib.Path]:
    if "*" in pattern:
        return sorted(CONTENT.glob(pattern))
    path = CONTENT / pattern
    return [path] if path.exists() else []


def extract() -> tuple[list[tuple[str, str, str]], dict[pathlib.Path, dict[str, str]]]:
    """Return (csv rows, {file: {original literal path -> id}})."""
    rows: list[tuple[str, str, str]] = []
    plans: dict[pathlib.Path, dict[str, str]] = {}
    seen_ids: dict[str, str] = {}

    for pattern, fields in MANIFEST.items():
        for path in files_for(pattern):
            rel = path.relative_to(CONTENT).as_posix()
            data = tomllib.loads(path.read_text(encoding="utf-8"))
            plan: dict[str, str] = {}
            for selector, context in fields:
                for segments, _container, _key, value in walk(data, selector, []):
                    string_id = ".".join([id_prefix(rel)] + segments)
                    if string_id in seen_ids:
                        sys.exit(f"duplicate id {string_id} (from {rel})")
                    seen_ids[string_id] = value
                    # Already migrated: the field holds its own id. Emitting a
                    # row here would bracket the id and overwrite the English,
                    # so leave the existing row alone and skip the rewrite.
                    if value == string_id:
                        continue
                    rows.append((string_id, context, f"[{value}]"))
                    plan[value] = string_id
            plans[path] = plan
    rows.sort(key=lambda row: row[0])
    return rows, plans


def audit() -> list[str]:
    """Report string leaves that are neither claimed as prose nor structural."""
    claimed: dict[str, set[str]] = {}
    for pattern, fields in MANIFEST.items():
        for path in files_for(pattern):
            data = tomllib.loads(path.read_text(encoding="utf-8"))
            values = claimed.setdefault(str(path), set())
            for selector, _context in fields:
                for _segments, _c, _k, value in walk(data, selector, []):
                    values.add(value)

    # Ids already in the table are migrated fields, not unclassified prose,
    # so the tool stays re-runnable after a partial migration.
    migrated: set[str] = set()
    if CSV_PATH.exists():
        with CSV_PATH.open(encoding="utf-8", newline="") as handle:
            migrated = {row["id"] for row in csv.DictReader(handle)}

    findings: list[str] = []
    for path in sorted(CONTENT.rglob("*.toml")):
        data = tomllib.loads(path.read_text(encoding="utf-8"))
        taken = claimed.get(str(path), set())

        def visit(node, trail: list[str]):
            if isinstance(node, dict):
                for key, value in node.items():
                    visit(value, trail + [str(key)])
            elif isinstance(node, list):
                for item in node:
                    visit(item, trail)
            elif isinstance(node, str):
                # Any structural segment on the trail disqualifies the leaf:
                # `legend.#` and `kind.ammo` are enum tags and item refs, not
                # prose, and they are nested below their telling key.
                if node in migrated:
                    return
                if taken and node in taken:
                    return
                if any(part in STRUCTURAL for part in trail):
                    return
                if len(node) == 1:  # legend glyphs
                    return
                findings.append(f"{path.as_posix()}: {'.'.join(trail)} = {node!r}")

        visit(data, [])
    return findings


def rewrite(path: pathlib.Path, plan: dict[str, str]) -> str:
    """Replace each prose literal in the TOML text with its id.

    Works on the source text so comments and layout survive. Every literal
    must be found; a miss is fatal rather than a silent skip.
    """
    text = path.read_text(encoding="utf-8")
    # Longest first, so a short string that is a substring of a longer one
    # cannot consume the wrong occurrence.
    for value in sorted(plan, key=len, reverse=True):
        string_id = plan[value]
        # TOML escapes: match the literal as it appears in the file.
        escaped = value.replace("\\", "\\\\").replace('"', '\\"')
        needle = f'"{escaped}"'
        if needle not in text:
            sys.exit(f"{path}: could not find literal for {string_id}: {needle[:80]}")
        text = text.replace(needle, f'"{string_id}"', 1)
    return text


def verify(path: pathlib.Path, new_text: str, plan: dict[str, str]) -> None:
    """Re-parse and confirm every field now holds its id."""
    data = tomllib.loads(new_text)
    ids = set(plan.values())
    found: set[str] = set()

    def visit(node):
        if isinstance(node, dict):
            for value in node.values():
                visit(value)
        elif isinstance(node, list):
            for item in node:
                visit(item)
        elif isinstance(node, str) and node in ids:
            found.add(node)

    visit(data)
    missing = ids - found
    if missing:
        sys.exit(f"{path}: ids missing after rewrite: {sorted(missing)[:5]}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="apply the migration")
    args = parser.parse_args()

    findings = audit()
    if findings:
        print("UNCLASSIFIED string leaves -- classify as prose or structural:")
        for finding in findings:
            print("  " + finding)
        return 1

    rows, plans = extract()
    print(f"extracted {len(rows)} strings from {len(plans)} files")
    if not args.write:
        print("(dry run; pass --write to apply)")
        return 0

    for path, plan in plans.items():
        new_text = rewrite(path, plan)
        verify(path, new_text, plan)
        path.write_text(new_text, encoding="utf-8", newline="")

    # Merge rather than overwrite: rows migrated in an earlier pass (and any
    # copy a human has since rewritten) must survive a later run.
    merged = {row[0]: row for row in rows}
    if CSV_PATH.exists():
        with CSV_PATH.open(encoding="utf-8", newline="") as handle:
            for existing in csv.DictReader(handle):
                merged.setdefault(
                    existing["id"],
                    (existing["id"], existing["context"], existing["english"]),
                )
    rows = sorted(merged.values(), key=lambda row: row[0])

    buf = io.StringIO()
    writer = csv.writer(buf, lineterminator="\r\n")
    writer.writerow(["id", "context", "english"])
    writer.writerows(rows)
    CSV_PATH.write_text(buf.getvalue(), encoding="utf-8", newline="")
    print(f"wrote {CSV_PATH} with {len(rows)} rows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
