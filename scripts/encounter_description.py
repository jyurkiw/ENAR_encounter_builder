"""Compiles an encounter description, for the encounter designer.

    python encounter_description.py COMPILER_DIR JOB_JSON

Run in the encompile project's environment (uv run --project COMPILER_DIR). JOB_JSON is
written by the designer:

    {"title": the encounter's title, "root": the resource folder (for messages),
     "out": the folder to write to, "dpi": for the page images,
     "monsters": [{"ref": an id, "file": monster XML, "monster": its id}, ...],
     "parties": [{"title": "Level 3, 5 PCs",
                  "rosters": [{"group", "difficulty", "summary",
                               "rows": [{"ref", "name", "count", "cr", "xp"}]}]}],
     "bestiary": [{"ref", "heading": a name to print above the stat block, or null}]}

The description is a supplement written to OUT/description.xml and compiled by encompile
(no cover or contents page): a chapter with a section per party (level and PCs) and in it a
table per group's roster (Name, Number, CR, AC, HP, XP; AC and HP read from the stat blocks),
then a Bestiary chapter with the chosen stat blocks, each under its heading. pdflatex writes
OUT/description.pdf, and each page becomes OUT/page0.png, page1.png, ...

A stat block may carry on into the next column but not onto the next page: after each build
the PDF is checked (see crossings()), a page break goes in before the first stat block that
runs onto another page, and it's built again, until none does. A stat block longer than a
page can't help it, and gets a warning.

Prints "PDF <path>" and one "PNG <path>" line per page, "warning: ..." lines on stderr; exits
non-zero on failure, with the reason on stderr.

The encounter designer embeds this file and writes it to its cache folder; edit it in the
encounter-designer repository (scripts/encounter_description.py).
"""
import json
import os
import re
import shutil
import subprocess
import sys
from xml.sax.saxutils import escape, quoteattr

# encompile relates paths with os.path.relpath, which raises across Windows drives.
_relpath = os.path.relpath


def _safe_relpath(path, start=os.curdir):
    try:
        return _relpath(path, start)
    except ValueError:
        return os.path.abspath(path)


os.path.relpath = _safe_relpath

sys.path.insert(0, os.path.abspath(sys.argv[1]))  # encompile is imported from its folder

from encompile.loader import CompileError, Library  # noqa: E402  (after the patch)
from encompile.monster import Block  # noqa: E402
from encompile.supplement import Supplement  # noqa: E402


def references(monsters):
    return ''.join(f'\n        <monster id={quoteattr(m["ref"])} file={quoteattr(os.path.abspath(m["file"]))} '
                   f'monster={quoteattr(m["monster"])}/>' for m in monsters)


def supplement(chapters, monsters):
    names = ''.join(f'\n        <ch name={quoteattr(name)}/>' for name, _ in chapters)
    body = ''.join(text for _, text in chapters)
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<supplement type="encounters">{body}
    <reference>{references(monsters)}
    </reference>
    <chapterOrder>{names}
    </chapterOrder>
</supplement>
'''


def stats(job, out, warnings):
    """{ref: (AC, HP)} as the stat blocks print them; a dash where a stat block can't say."""
    path = os.path.join(out, 'stats.xml')
    with open(path, 'w', encoding='utf-8') as f:
        f.write(supplement([], job['monsters']))
    doc = Library(job.get('root') or out, validate=False).load(path)
    found = {}
    for m in job['monsters']:
        entry = doc.refs[m['ref']]
        try:
            block = Block(entry.monster(), entry.target.root, entry.target)
            ac = block.num(block.h.find('armorClass'), what='armor class').words()
            hp = block.hit_points()[0].words()
            found[m['ref']] = (ac, hp)
        except (CompileError, AttributeError, KeyError, TypeError, ValueError) as e:
            warnings.append(f'{m["monster"]}: no AC or HP for the summary ({e})')
            found[m['ref']] = ('—', '—')
    return found


def cell(value):
    return f'<td>{escape(str(value))}</td>'


def encounter_chapter(job, acs):
    sections, tables, n = [], [], 0
    for pi, party in enumerate(job['parties']):
        sections.append(f'''
        <section name="party-{pi}">
            <title>{escape(party["title"])}</title>
        </section>''')
        for r in party['rosters']:
            n += 1
            rows = ''.join(
                '\n                    <tr>' + ''.join(cell(v) for v in (
                    row['name'], row['count'], row['cr'] or '?', *acs.get(row.get('ref'), ('—', '—')),
                    row['xp'] if row.get('xp') is not None else '?')) + '</tr>'
                for row in r['rows'])
            tables.append(f'''
        <table id="roster-{n}">
            <header>
                <th>Name</th><th>Number</th><th>CR</th><th>AC</th><th>HP</th><th>XP</th>
            </header>
            <tableBody>{rows}
            </tableBody>
        </table>''')
            lines = [f'Intended difficulty: {r["difficulty"]}.' if r.get('difficulty') else '', r.get('summary') or '']
            lead = ' '.join(x for x in lines if x)
            lead = f'\n            <p>{escape(lead)}</p>' if lead else ''
            sections.append(f'''
        <section name="roster-{n}">
            <subtitle>{escape(r["group"])}</subtitle>{lead}
            <tableLink id="roster-{n}"/>
        </section>''')
    return f'''
    <chapter name="encounter" kind="unnumbered">
        <chapterTitle>{escape(job["title"])}</chapterTitle>{"".join(sections)}{"".join(tables)}
    </chapter>'''


def bestiary_chapter(job, breaks=()):
    """The stat blocks, each in a section under its heading; a page break before each
    stat block in breaks (indexes into job['bestiary'])."""
    blocks = ''.join(
        ('\n        <pageEnd/>' if i in breaks else '') + f'''
        <section name="block-{i}">{f"{chr(10)}            <title>{escape(b['heading'])}</title>" if b.get('heading') else ''}
            <monsterStatblock ref={quoteattr(b["ref"])}/>
        </section>''' for i, b in enumerate(job['bestiary']))
    return f'''
    <chapter name="bestiary" kind="unnumbered">
        <chapterTitle>Bestiary</chapterTitle>{blocks}
    </chapter>'''


# The fill of dndbook's stat block box (as scripts/statblock.py crops by).
PARCHMENT = (0.992, 0.945, 0.863)


def crossings(pdf_path):
    """The pages each stat block is on, in the PDF's order; None when the PDF's boxes
    can't be told apart.

    Each stat block is drawn as parchment boxes: a short title strip with the body box
    right under it where the block starts, then one box per column it carries on into.
    So, in reading order (page, column, top), a box that touches the one under it is a
    block's start, and any other box carries on the block before."""
    import pymupdf
    doc = pymupdf.open(pdf_path)
    boxes = []                          # (page, column, top, bottom)
    for pno, page in enumerate(doc):
        for d in page.get_drawings():
            fill, r = d.get('fill'), d['rect']
            if fill is not None and r.width > 100 and all(abs(a - b) < 0.01 for a, b in zip(fill, PARCHMENT)):
                boxes.append((pno, int(r.x0 > page.rect.width / 2), r.y0, r.y1))
    boxes.sort()
    pages, i = [], 0                    # pages[n]: the pages block n is on
    while i < len(boxes):
        p, c, top, bottom = boxes[i]
        starts = i + 1 < len(boxes) and boxes[i + 1][:2] == (p, c) and abs(boxes[i + 1][2] - bottom) < 1.5
        if starts:
            pages.append({p})
            i += 2
            continue
        if not pages:
            return None                 # a box carrying on before any block started
        pages[-1].add(p)
        i += 1
    return pages


def pdflatex(tex, warnings, passes=2):
    exe = shutil.which('pdflatex')
    if not exe:
        raise CompileError('pdflatex not found; the description needs LaTeX with the dndbook class')
    folder, name = os.path.split(tex)
    pdf = os.path.splitext(tex)[0] + '.pdf'
    if os.path.exists(pdf):
        os.remove(pdf)              # so an old PDF can't pass for this build's
    for _ in range(passes):         # twice, as encompile does, for references
        subprocess.run([exe, '-interaction=nonstopmode', name], cwd=folder, capture_output=True,
                       text=True, encoding='utf-8', errors='replace')
    try:
        with open(os.path.splitext(tex)[0] + '.log', encoding='utf-8', errors='replace') as f:
            log = f.read()
    except OSError:
        log = ''
    if not os.path.exists(pdf):
        first = re.findall(r'^! (.*)$', log, re.M)[:3]
        raise CompileError('pdflatex made no PDF' + (': ' + '; '.join(first) if first else ''))
    errors = re.findall(r'^! (.*)$', log, re.M)
    if errors:
        warnings.append(f'pdflatex reported {len(errors)} error(s), the PDF may be incomplete; first: '
                        + '; '.join(dict.fromkeys(e.strip() for e in errors[:3])))
    return pdf


def main():
    with open(sys.argv[2], encoding='utf-8') as f:
        job = json.load(f)
    out = os.path.abspath(job['out'])
    os.makedirs(out, exist_ok=True)
    for name in os.listdir(out):
        if re.fullmatch(r'page\d+\.png|description\.(pdf|tex|aux|log|xml)|stats\.xml', name):
            os.remove(os.path.join(out, name))
    warnings = []
    try:
        acs = stats(job, out, warnings)
        encounter = encounter_chapter(job, acs)
        bestiary = job['bestiary']
        breaks, too_long = set(), set()
        for _ in range(len(bestiary) + 1):
            chapters = [('encounter', encounter)]
            if bestiary:
                chapters.append(('bestiary', bestiary_chapter(job, breaks)))
            xml = os.path.join(out, 'description.xml')
            with open(xml, 'w', encoding='utf-8') as f:
                f.write(supplement(chapters, job['monsters']))
            lib = Library(job.get('root') or out, validate=False)
            doc = lib.load(xml)
            text = Supplement(doc, out, contents=False).render()
            tex = os.path.join(out, 'description.tex')
            with open(tex, 'w', encoding='utf-8', newline='\n') as f:
                f.write(text)
            build_warnings = list(lib.warnings)
            pdf = pdflatex(tex, build_warnings, passes=1)
            if not bestiary:
                break
            found = crossings(pdf)
            if found is None or len(found) != len(bestiary):
                build_warnings.append("couldn't find every stat block in the PDF, so some may run "
                                      "onto the next page")
                break
            # A stat block on more than one page gets a page break before it. The
            # bestiary's first starts a page already, as does one with a break before
            # it: those are longer than a page.
            crossing = [n for n, on in enumerate(found) if len(on) > 1]
            too_long = {n for n in crossing if n == 0 or n in breaks}
            fixable = [n for n in crossing if n not in too_long]
            if not fixable:
                break
            breaks.add(fixable[0])      # the first: a break moves everything after it
        pdf = pdflatex(tex, build_warnings, passes=1)   # the second pass, for references
        warnings += dict.fromkeys(build_warnings)
        for n in sorted(too_long):
            b = bestiary[n]
            name = b.get('heading') or next(m['monster'] for m in job['monsters'] if m['ref'] == b['ref'])
            warnings.append(f'{name}: the stat block is longer than a page, so it runs onto the next one')
    except CompileError as e:
        for w in warnings:
            print(f'warning: {w}', file=sys.stderr)
        print(f'error: {e}', file=sys.stderr)
        return 1

    import pymupdf

    doc = pymupdf.open(pdf)
    pages = []
    for i, page in enumerate(doc):
        path = os.path.join(out, f'page{i}.png')
        page.get_pixmap(dpi=int(job.get('dpi') or 110)).save(path)
        pages.append(path)
    for w in warnings:
        print(f'warning: {w}', file=sys.stderr)
    print(f'PDF {pdf}')
    for path in pages:
        print(f'PNG {path}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
