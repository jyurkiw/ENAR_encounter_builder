"""Writes an encounter's monsters as Foundry VTT actor JSON, for the encounter designer.

    python foundry_export.py COMPILER_DIR JOB_JSON

Run in the encompile project's environment (uv run --project COMPILER_DIR). JOB_JSON is
written by the designer:

    {"out": folder, "root": the resource folder (for messages),
     "actors": [{"file": monster XML, "monster": its id,
                 "name": actor name or null, "token": token name or null,
                 "out": file name}, ...]}

Each actor is built by encompile.foundry (the same JSON `python -m encompile.foundry` writes),
then renamed: "name" replaces the actor's name (its character sheet) and "token" its prototype
token's name. null keeps the stat block's. A monster used under several names is built once.

Prints one JSON report on stdout: {"written": [paths], "warnings": [...], "errors": [...]}.
A monster that can't be built is an error and the rest are still written.

The encounter designer embeds this file and writes it to its cache folder; edit it in the
encounter-designer repository (scripts/foundry_export.py).
"""
import json
import os
import sys

# encompile relates paths with os.path.relpath, which raises across Windows drives.
_relpath = os.path.relpath


def _safe_relpath(path, start=os.curdir):
    try:
        return _relpath(path, start)
    except ValueError:
        return os.path.abspath(path)


os.path.relpath = _safe_relpath

sys.path.insert(0, os.path.abspath(sys.argv[1]))  # encompile is imported from its folder

from encompile import foundry, foundry_spells  # noqa: E402  (after the patch)
from encompile.loader import CompileError  # noqa: E402


def main():
    with open(sys.argv[2], encoding="utf-8") as f:
        job = json.load(f)
    out, root = job["out"], job.get("root") or None
    report = {"written": [], "warnings": [], "errors": []}

    cache = None
    if os.path.isfile(foundry_spells.SPELLS_ZIP):
        try:
            cache, problems = foundry_spells.use_export(foundry_spells.SPELLS_ZIP)
            report["warnings"] += [f"Spells.zip: {w}" for w in problems]
        except CompileError as e:
            report["warnings"].append(f"Spells.zip: {e}; spells are exported without cached copies")

    built = {}  # (file, monster) -> actor dict, or None when it failed
    casts = False
    os.makedirs(out, exist_ok=True)
    for a in job["actors"]:
        key = (a["file"], a["monster"])
        if key not in built:
            try:
                [(_, actor, warnings)], lib = foundry.actors(a["file"], a["monster"], root, True, cache)
                built[key] = actor
                report["warnings"] += [w for w in lib.warnings + warnings if w not in report["warnings"]]
                casts = casts or any(v["type"] == "cast" for i in actor["items"]
                                     for v in i["system"].get("activities", {}).values())
            except (CompileError, OSError) as e:
                built[key] = None
                report["errors"].append(f"{a['monster']} ({a['file']}): {e}")
        actor = built[key]
        if actor is None:
            continue
        actor = json.loads(json.dumps(actor))  # a copy per name
        if a.get("name"):
            actor["name"] = a["name"]
        if a.get("token"):
            actor["prototypeToken"]["name"] = a["token"]
        elif a.get("name"):
            actor["prototypeToken"]["name"] = a["name"]
        path = os.path.join(out, a["out"])
        try:
            with open(path, "w", encoding="utf-8") as f:
                json.dump(actor, f, indent=2, ensure_ascii=False)
                f.write("\n")
            report["written"].append(path)
        except OSError as e:
            report["errors"].append(f"{path}: {e}")

    if cache is None and casts:
        report["warnings"].append(
            f"No Spells.zip in {foundry_spells.COMPILER_DIR}: spells are cast without cached copies, so "
            f"Foundry's Additional Spells list fills in only as each spell is first cast")
    print(json.dumps(report))
    return 0


if __name__ == "__main__":
    sys.exit(main())
