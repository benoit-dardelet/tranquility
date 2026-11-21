#!/usr/bin/env python3
"""Generate phone/src/ui/font.rs from a PSF2 console font (Terminus 8x16,
SIL OFL 1.1) plus a few hand-drawn symbols the UI uses."""
import gzip, re, sys, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
PSF = sys.argv[1] if len(sys.argv) > 1 else "/usr/share/consolefonts/Uni2-Terminus16.psf.gz"

def load_psf2(path):
    d = gzip.open(path).read() if path.endswith(".gz") else open(path, "rb").read()
    if d[:2] == b"\x36\x04":  # PSF1: 8 px wide, u16 unicode table
        mode, h = d[2], d[3]
        assert h == 16, h
        n = 512 if mode & 1 else 256
        glyphs = [d[4 + i*h: 4 + (i+1)*h] for i in range(n)]
        t = d[4 + n*h:]
        m, g, i, seq = {}, 0, 0, False
        while i + 1 < len(t) and g < n:
            u = int.from_bytes(t[i:i+2], "little"); i += 2
            if u == 0xFFFF:
                g += 1; seq = False
            elif u == 0xFFFE:
                seq = True
            elif not seq:
                m.setdefault(u, glyphs[g])
        return m
    assert d[:4] == bytes.fromhex("72b54a86"), "not PSF"
    hsize, flags, n, bpg, h, w = [int.from_bytes(d[i:i+4], "little") for i in range(8, 32, 4)]
    assert (w, h) == (8, 16), (w, h)
    glyphs = [d[hsize + i*bpg: hsize + (i+1)*bpg] for i in range(n)]
    table = d[hsize + n*bpg:]
    m = {}
    i = 0
    for g in range(n):
        end = table.index(b"\xff", i)
        entry = table[i:end]
        i = end + 1
        # single codepoints before the first 0xFE (sequences ignored)
        single = entry.split(b"\xfe")[0]
        for ch in single.decode("utf-8", "ignore"):
            m.setdefault(ord(ch), glyphs[g])
    return m

def bits(rows):
    """rows: 16 strings of 8 chars ('#' = ink)."""
    assert len(rows) == 16, len(rows)
    return bytes(int(r.ljust(8).replace("#", "1").replace(".", "0").replace(" ", "0")[:8], 2) for r in rows)

def pad(rows):
    top = (16 - len(rows)) // 2
    return ["........"] * top + rows + ["........"] * (16 - len(rows) - top)

# Symbols the UI uses that the console font may lack. Drawn for 8x16.
CUSTOM = {
    "✉": pad(["........", "########", "##....##", "#.#..#.#", "#..##..#", "#......#", "########"]),
    "☎": pad(["..####..", ".######.", "##....##", "..####..", ".##..##.", ".######.", ".######."]),
    "◷": pad(["..####..", ".#.#..#.", "#..#...#", "#..###.#", "#......#", ".#....#.", "..####.."]),
    "✈": pad(["...##...", "...##...", "..####..", "########", "...##...", "...##...", "..####.."]),
    "♪": pad(["....##..", "....#.#.", "....#...", "....#...", "..###...", ".####...", "..##...."]),
    "♫": pad(["..######", "..#....#", "..#....#", "..#....#", "###..###", "###..###", ".#....#."]),
    "⚠": pad(["...##...", "..#..#..", "..#..#..", ".#.##.#.", ".#.##.#.", "#......#", "#..##..#", "########"]),
    "✓": pad([".......#", "......##", ".....##.", "#...##..", "##.##...", ".###....", "..#....."]),
    "✗": pad(["#.....#.", ".#...#..", "..#.#...", "...#....", "..#.#...", ".#...#..", "#.....#."]),
    "✎": pad(["......##", ".....#.#", "....#.#.", "...#.#..", "..#.#...", ".##.#...", ".###....", "####...."]),
    "◆": pad(["...##...", "..####..", ".######.", "########", ".######.", "..####..", "...##..."]),
    "▪": pad(["..####..", "..####..", "..####..", "..####.."]),
    "↗": pad(["...#####", "......##", ".....#.#", "....#..#", "...#....", "..#.....", ".#......"]),
    "↙": pad(["......#.", ".....#..", "....#...", "#..#....", "#.#.....", "##......", "#####..."]),
    "▼": pad(["########", ".######.", "..####..", "...##..."]),
    "…": pad(["........", "........", "........", "........", "##.##.##"]),
    "●": pad(["..####..", ".######.", "########", "########", ".######.", "..####.."]),
    "·": pad(["...##...", "...##..."]),
    "−": pad(["........", "........", "........", ".######.", "........", "........", "........"]),
    "×": pad([".#....#.", "..#..#..", "...##...", "...##...", "..#..#..", ".#....#."]),
    "÷": pad(["...##...", "........", ".######.", "........", "...##..."]),
    "▂": ["........"] * 13 + ["########"] * 3,
    "▄": ["........"] * 8 + ["########"] * 8,
    "▆": ["........"] * 4 + ["########"] * 12,
    "█": ["########"] * 16,
    "▀": ["########"] * 8 + ["........"] * 8,
}
REPLACEMENT = bits(pad(["########", "#......#", "#..##..#", "#.#..#.#", "#....#.#", "#...#..#", "#......#", "#...#..#", "########"]))

def main():
    m = load_psf2(PSF)
    # characters used anywhere in the phone stack's sources
    used = set()
    for f in (ROOT / "phone" / "src").rglob("*.rs"):
        if f.name == "font.rs":
            continue
        used |= {c for c in f.read_text() if ord(c) > 0x7E}
    out = {}
    for cp, g in m.items():
        if 0x20 <= cp <= 0x7E or 0xA0 <= cp <= 0x24F or 0x370 <= cp <= 0x3FF or 0x2500 <= cp <= 0x259F or chr(cp) in used:
            out[cp] = g
    for ch, rows in CUSTOM.items():
        if ord(ch) not in out or ch in "▂▄▆█▀":
            out[ord(ch)] = bits(rows)
    missing = sorted(c for c in used if ord(c) not in out and ord(c) >= 0x20 and not 0x300 <= ord(c) <= 0x36F)
    lines = [
        "//! 8x16 bitmap font. Generated by tools/genfont.py — do not edit.",
        "//!",
        "//! Glyphs from Terminus Font (Dimitar Zhekov, SIL Open Font License 1.1),",
        "//! plus UI symbols drawn for this project.",
        "",
        f"pub const REPLACEMENT: [u8; 16] = {list(REPLACEMENT)};",
        "",
        "/// (codepoint, rows) sorted by codepoint; each row is 8 pixels, MSB left.",
        f"pub static GLYPHS: [(u32, [u8; 16]); {len(out)}] = [",
    ]
    for cp in sorted(out):
        lines.append(f"    (0x{cp:04X}, {list(out[cp])}),")
    lines += ["];", "", "pub fn glyph(c: char) -> &'static [u8; 16] {",
              "    match GLYPHS.binary_search_by_key(&(c as u32), |g| g.0) {",
              "        Ok(i) => &GLYPHS[i].1,",
              "        Err(_) => &REPLACEMENT,",
              "    }", "}", ""]
    (ROOT / "phone/src/ui/font.rs").write_text("\n".join(lines))
    print(f"{len(out)} glyphs; not covered (drawn as a box): {''.join(missing)}")

main()
