# Encounter Designer

A desktop app for building Enigma Narrative encounter files (`encounter_schema.xsd`): the app version of the old XP spreadsheet. Pick monsters from your monster folders, drop them into rosters, and see each roster's XP against the target range for its party, worked out from its group's intended difficulty.

Written in Rust with [iced](https://iced.rs) 0.14.

## Running it

```sh
cargo run             # debug build, then run it
cargo run --release   # release build, then run it
```

The first launch writes a config file with defaults (see **Configuration**).

To build without running:

| Build | Command | Executable |
|---|---|---|
| Debug | `cargo build` | `target\debug\encounter-designer.exe` |
| Release | `cargo build --release` | `target\release\encounter-designer.exe` |

To build both the debug and the release executable in one go (Cargo builds one profile per command):

```powershell
.\build.ps1          # cargo build, then cargo build --release
.\build.ps1 -Test    # run the tests first, and build only if they pass
```

The release build is `target\release\encounter-designer.exe`: smaller and faster, and a single file you can copy anywhere or pin to the taskbar.

To draw stat blocks in the monster viewer, the app also needs:

- [uv](https://docs.astral.sh/uv/) on `PATH` (or set `uv_command` in the config file)
- the `encompile` compiler from the enigma-narrative repository (`compiler/`)
- LaTeX (`pdflatex`) with the [D&D 5e LaTeX template](https://github.com/rpgtex/DND-5e-LaTeX-Template) installed, for example in `%USERPROFILE%\texmf\tex\latex\dnd`

Without them everything else works; the viewer shows why it couldn't draw a stat block.

## Using it

The window has a ribbon along the top (New, Open, Save, Save All, Export to Foundry, Configuration), the monster browser on the left, the open encounters in the middle, and the monster viewer on the right.

**Monster browser.** Lists the monsters in the SRD folder and your monster folders as `Name (CR X)`. Folder headings and files holding several monsters are collapsible headings; the filter box searches names and ids. A monster's file is shown only when another file has a monster of the same name. Click a monster to see its stat block; drag it onto a roster to add it.

**Encounter editor.** One tab per open encounter.

- The header sets the encounter id and the party level and number of PCs.
- Each **group** has its own **intended difficulty**: a range from Low to High (2024 DMG), set with the buttons under the group's name. Each difficulty covers the XP above the budget below it, up to its own: for 5 level-3 PCs, Moderate is 751 to 1,125 XP and Moderate to High is 751 to 2,000.
- Each group has a base roster (for the encounter's party) and any number of **alternate rosters** for other levels and party sizes, all measured against the group's difficulty range. "+ Alternate roster" copies the base roster one level up.
- Each roster lists its monsters with CR, count and XP (read from the monster files), then its total, its rating, and where it stands against its target: "Add 451 – 1,700 XP", "In range · 650 XP to spare" or "225 XP over".
- Click a roster to make it the one **Add to roster** (in the monster viewer) fills; it's outlined and marked "Adding here". Dragging a monster onto a roster adds it there instead.
- **Local names** (beside each monster in a roster) opens two fields for a display name and short name used in that roster only, in place of the reference's. A dot on the button means the row has some.
- **Add named copy** lists the same stat block again below the row, for several named monsters of one kind (two bandit captains, say). Each of those entries then needs its own local display name: until it has one, its row says "Local name needed" in red and keeps its fields open. Adding a monster that's already there (by dragging or Add to roster) counts up its unnamed entry instead.
- **Monster references** sets the names the encounter prints for its monsters, and offers to remove references no roster uses.
- Problems the schema would reject (an empty roster, two alternate rosters for the same party) are listed at the bottom.

Shortcuts: Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S (Save All). Tab and Shift+Tab move between text fields (from a local name to its short name, then on to the next row).

**Export to Foundry.** Writes the active encounter's monsters as Foundry VTT actor files (dnd5e), using the compiler's `encompile.foundry`: the same JSON as `python -m encompile.foundry`, one `fvtt-Actor-….json` per monster, for Foundry's Import Data (right-click an NPC actor in the Actors sidebar). The modal lists the files first; pick or type a folder (made if it doesn't exist; `res://` works), then Okay. Each stat block is written once, from every roster in every group. A monster used under its own names gets a file per name instead: the display name (the roster entry's local one, else its reference's) goes on the actor's sheet, and the short name (else the display name) on its prototype token, so `fvtt-Actor-bandit-captain-red-fang.json` is "Red Fang" with a "Fang" token. The stat block itself isn't changed. Afterwards the modal lists what was written and the compiler's warnings; a monster that couldn't be built is listed and the rest are still written. The folder is remembered for next time. Spells get cached copies when the compiler folder has `Spells.zip` (see the compiler's README).

**Monster viewer.** Shows the selected monster's stat block as the compiler prints it, cropped to the stat block. The first view of a monster compiles it (a few seconds); after that it's cached until the monster's file changes. Recompile forces a fresh build.

## Configuration

The config file is `%APPDATA%\encounter-designer\config.toml`, edited with the Configuration button or by hand:

| Setting | Meaning |
|---|---|
| `resource_dir` | The folder `res://` paths resolve against (default `e:/Data/enigma-narrative/`) |
| `srd_monster_dir` | The SRD monster folder, and whether its subfolders are included |
| `monster_dirs` | Further monster folders; in the list, a trailing `*` marks a folder whose subfolders are included |
| `compiler_dir` | The encompile compiler (default `res://compiler`) |
| `uv_command` | How to run uv (default `uv`) |
| `foundry_export_dir` | The folder the last Foundry export went to |

Paths can be absolute or start with `res://`. Settings added in newer versions are written into an existing file, with their defaults, the next time it's loaded.

Stat block images, their LaTeX and PDF, and the compiler's Python environment live in `%LOCALAPPDATA%\encounter-designer`, never in the repositories.

## Encounter files

Encounters follow `encounter_schema.xsd` (in enigma-narrative's `schemas/`): an `<encounter>` with `id`, `level` and `numPCs`, one or more `<group>`s of rosters, each with its own optional `minDifficulty`/`maxDifficulty` (low, moderate or high; default moderate) and roster `<monster>`s with optional local `displayName`/`shortName`, and a `<reference>` block mapping roster ids to monster files. Files saved before the range moved onto groups have it on `<encounter>`; it's read as the range for every group. `encounters.xml` is a sample. Saving rewrites the whole file, so XML comments are not kept.

## Tests

```sh
cargo test                  # unit tests and headless UI tests
cargo test -- --ignored     # also the real compiler: drawing the SRD lich, exporting a renamed goblin
```

The UI tests use [`iced_test`](https://docs.rs/iced_test/0.14.0) to drive the real view without a window: they click, type and drag through it, feed the messages back through `update`, and run every task the app returns. File dialogs are scripted and the stat block compiler and Foundry exporter are faked (see `src/ui_tests.rs`). `.cargo/config.toml` makes them render on the CPU, which keeps the suite under a second.

Native file dialogs and the real window's behaviour aren't covered; those are tested by using the app.

## Layout

| Path | What's there |
|---|---|
| `src/main.rs` | The app: state, messages, ribbon, panels, monster browser and viewer |
| `src/editor.rs` | The encounter editor |
| `src/encounter.rs` | The encounter file model: reading, writing and editing |
| `src/xp.rs` | XP by CR, the 2024 DMG budgets, difficulty ranges |
| `src/monsters.rs` | Scanning monster folders |
| `src/statblock.rs` | Compiling and caching stat block images |
| `src/foundry.rs`, `src/export_modal.rs` | Foundry export: which files, running the export, and its modal |
| `src/config.rs`, `src/config_modal.rs` | The config file and its modal |
| `src/dialogs.rs` | File dialogs, behind a seam tests can script |
| `src/ui_tests.rs` | Headless UI tests |
| `scripts/statblock.py` | Run by the app through uv: compiles one monster with encompile and crops its stat block to PNGs |
| `scripts/foundry_export.py` | Run by the app through uv: builds actors with `encompile.foundry`, renames them and writes them |
| `tools/make_icon.py` | Draws the icon from its 16x16 pixel grid into `assets/` |
| `build.rs`, `assets/app.rc` | Embed the icon in the Windows `.exe` |
| `build.ps1` | Builds debug and release (optionally testing first) |

To change the icon, edit the grid in `tools/make_icon.py` and run `python tools/make_icon.py`.
