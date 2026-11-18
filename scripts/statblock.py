"""Renders one monster's stat block to cropped PNGs, for the encounter designer.

    python statblock.py COMPILER_DIR XML MONSTER_ID OUT_DIR DPI

Run in the encompile project's environment (uv run --project COMPILER_DIR). Compiles the
monster's stat block alone, in one column, to a PDF in OUT_DIR, then crops each
page to the stat block's parchment box and writes page0.png, page1.png, ...
Prints one "PNG <path>" line per image; exits non-zero on failure, with the
reason on stderr.

The encounter designer embeds this file and writes it to its cache folder; edit
it in the encounter-designer repository (scripts/statblock.py).
"""
import os
import sys

# encompile relates paths with os.path.relpath, which raises across Windows
# drives (the cache lives on C:, the monster files may be on E:). Fall back to
# the absolute path, which every caller can use just as well.
_relpath = os.path.relpath


def _safe_relpath(path, start=os.curdir):
    try:
        return _relpath(path, start)
    except ValueError:
        return os.path.abspath(path)


os.path.relpath = _safe_relpath

sys.path.insert(0, os.path.abspath(sys.argv[1]))  # encompile is imported from its folder

from encompile import cli  # noqa: E402  (after the patch)

# The viewer wants the stat block alone (no lore, lairs or images, as -c 2
# prints) but in the book's single column, which fits a side panel. -c 2 makes
# a wrapper of <monsterStatblock ... columns="2"/>; drop the columns="2".
_monster_bestiary = cli.monster_bestiary


def _single_column_statblocks(src, which, folder, columns=None):
    path = _monster_bestiary(src, which, folder, "2")
    with open(path, encoding="utf-8") as f:
        text = f.read()
    with open(path, "w", encoding="utf-8") as f:
        f.write(text.replace(' columns="2"', ""))
    return path


cli.monster_bestiary = _single_column_statblocks

# The fill of dndbook's stat block box.
PARCHMENT = (0.992, 0.945, 0.863)


def is_box(drawing):
    fill = drawing.get("fill")
    return fill is not None and all(abs(a - b) < 0.01 for a, b in zip(fill, PARCHMENT))


def main():
    xml, monster, out, dpi = sys.argv[2], sys.argv[3], sys.argv[4], int(sys.argv[5])
    os.makedirs(out, exist_ok=True)
    for name in os.listdir(out):
        if name.endswith(".png"):
            os.remove(os.path.join(out, name))

    pdf = os.path.join(out, "statblock.pdf")
    code = cli.main([xml, "-m", monster, "-c", "2", "--no-validate", "-o", pdf])
    if code != 0 or not os.path.exists(pdf):
        print(f"error: the compiler failed (exit {code})", file=sys.stderr)
        return code or 1

    import pymupdf

    doc = pymupdf.open(pdf)
    written = []
    for page in doc:
        boxes = [d["rect"] for d in page.get_drawings() if is_box(d)]
        # A long stat block carries on in the next column (or page): one
        # image per column piece, in reading order, which the viewer stacks
        # into one continuous column.
        columns = []
        for r in sorted(boxes, key=lambda r: r.x0):
            if columns and abs(columns[-1].x0 - r.x0) < 20:
                columns[-1] |= r
            else:
                columns.append(pymupdf.Rect(r))
        for clip in columns:
            clip = (clip + (-3, -3, 3, 3)) & page.rect
            path = os.path.join(out, f"page{len(written)}.png")
            page.get_pixmap(dpi=dpi, clip=clip).save(path)
            written.append(path)
    if not written:  # no box found (a template change?): show the whole first page
        path = os.path.join(out, "page0.png")
        doc[0].get_pixmap(dpi=dpi).save(path)
        written.append(path)
    for path in written:
        print(f"PNG {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
