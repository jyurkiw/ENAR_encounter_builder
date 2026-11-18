"""Builds the Encounter Designer icon from the pixel grid below.

    python tools/make_icon.py

Writes assets/icon.png (256 px, used for the window and taskbar) and
assets/icon.ico (16 to 256 px, embedded in the .exe by build.rs). Every size is
the 16x16 grid scaled up with nearest-neighbour, so it stays pixel art.

The design: a crocodile head in profile, jaws open to the right like a "<",
inside a rounded square.
"""
import os
import sys

from PIL import Image

GRID = """
..############..
.##############.
###EE#######NN##
##EYKEGGGGGGGG##
##GGGGGGGGGGGGG#
##GGGGGGGGGWRWR#
##GGGGGGWRWRRRR#
##GGGGWRRRRRRRR#
##GGGGRRRRRRRRR#
##GGGGWRRRRRRRR#
##GGGGGGWRWRRRR#
##GGGGGGGGGWRWR#
##GGGGGGGGGGGG##
################
.##############.
..############..
"""

PALETTE = {
    ".": (0, 0, 0, 0),          # transparent (the rounded corners)
    "#": (24, 60, 48, 255),     # the square: deep swamp green
    "G": (110, 190, 70, 255),   # crocodile
    "E": (90, 160, 55, 255),    # the eye's brow ridge
    "N": (90, 160, 55, 255),    # nostril bump
    "Y": (245, 215, 60, 255),   # eye
    "K": (20, 20, 20, 255),     # pupil
    "W": (250, 250, 240, 255),  # teeth
    "R": (150, 40, 45, 255),    # inside the mouth
}

SIZES = [16, 24, 32, 48, 64, 128, 256]


def grid_image(grid=GRID):
    rows = [r for r in grid.strip().splitlines()]
    assert len(rows) == 16 and all(len(r) == 16 for r in rows), "the grid must be 16x16"
    img = Image.new("RGBA", (16, 16))
    for y, row in enumerate(rows):
        for x, ch in enumerate(row):
            img.putpixel((x, y), PALETTE[ch])
    return img


def main():
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")
    os.makedirs(root, exist_ok=True)
    base = grid_image()
    big = base.resize((256, 256), Image.NEAREST)
    big.save(os.path.join(root, "icon.png"))
    # Pillow writes each size from the image given, so hand it the 256 px one
    # and let it scale down; nearest-neighbour multiples of 16 stay crisp.
    big.save(os.path.join(root, "icon.ico"), sizes=[(s, s) for s in SIZES])
    if len(sys.argv) > 1:  # a preview path: the icon at a few sizes side by side
        sheet = Image.new("RGBA", (16 + 32 + 64 + 128 + 50, 140), (240, 240, 240, 255))
        x = 10
        for s in (16, 32, 64, 128):
            sheet.paste(base.resize((s, s), Image.NEAREST), (x, 140 - s - 6))
            x += s + 10
        sheet.save(sys.argv[1])
    print("wrote", os.path.normpath(root))


if __name__ == "__main__":
    main()
