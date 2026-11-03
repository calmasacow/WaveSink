"""Read Omarchy theme palettes into the shape src/themes/omarchy.ts bundles.

Two formats exist in the wild: a modern colors.toml with named roles, and
older themes that only ship terminal colours in alacritty.toml. Both are
reduced to the same key set WaveSink needs.
"""
import json
import os
import re
import sys

HEX = re.compile(r'^#[0-9a-fA-F]{6}$')
WANTED = [
    "accent", "selection", "muted",
    "background", "dark_background", "darker_background", "lighter_background",
    "foreground", "dark_foreground", "light_foreground", "bright_foreground",
    "red", "yellow", "orange", "green", "cyan", "blue", "magenta", "brown",
    "bright_red", "bright_yellow", "bright_green", "bright_cyan",
    "bright_blue", "bright_magenta",
]
# WaveSink's applyPalette() cannot render a theme without these.
REQUIRED = ["accent", "background", "foreground", "bright_foreground",
            "red", "yellow", "green"]


def parse_pairs(text):
    """key = "#rrggbb" pairs, ignoring comments and non-colour values."""
    out = {}
    for line in text.splitlines():
        line = line.split("#", 1)[0] if line.strip().startswith("#") else line
        m = re.match(r'''\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*['"]?(#[0-9a-fA-F]{6})['"]?''', line)
        if m:
            out.setdefault(m.group(1), m.group(2))
    return out


def srgb_to_linear(c):
    c /= 255
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def luminance(hexstr):
    r, g, b = (int(hexstr[i:i + 2], 16) for i in (1, 3, 5))
    return (0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g)
            + 0.0722 * srgb_to_linear(b))


def mix(hexstr, factor):
    """Lighten (factor>1) or darken (factor<1) while staying in range."""
    r, g, b = (int(hexstr[i:i + 2], 16) for i in (1, 3, 5))
    f = lambda v: max(0, min(255, round(v * factor)))
    return f"#{f(r):02x}{f(g):02x}{f(b):02x}"


def from_alacritty(path):
    """Older themes: map terminal colours onto the named roles."""
    text = open(path, encoding="utf-8", errors="replace").read()
    raw = parse_pairs(text)
    # Alacritty nests under [colors.primary] / [colors.normal] / [colors.bright];
    # section order is what distinguishes same-named keys.
    sections, current = {}, None
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("["):
            current = s.strip("[]").strip()
            sections[current] = {}
        elif current is not None:
            m = re.match(r'''\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*['"]?(#[0-9a-fA-F]{6})['"]?''', line)
            if m:
                sections[current][m.group(1)] = m.group(2)

    primary = sections.get("colors.primary", {})
    normal = sections.get("colors.normal", {})
    bright = sections.get("colors.bright", {})
    cursor = sections.get("colors.cursor", {})
    sel = sections.get("colors.selection", {})

    bg = primary.get("background") or raw.get("background")
    fg = primary.get("foreground") or raw.get("foreground")
    if not bg or not fg:
        return None

    dark = luminance(bg) <= 0.5
    # The accent a theme actually shows: its cursor, else a vivid normal colour.
    accent = (cursor.get("cursor") or cursor.get("background")
              or raw.get("accent") or normal.get("blue") or normal.get("cyan")
              or bright.get("blue") or fg)
    colors = {
        "accent": accent,
        "selection": sel.get("background") or mix(bg, 1.8 if dark else 0.92),
        "muted": normal.get("black") if dark else bright.get("black"),
        "background": bg,
        "dark_background": mix(bg, 0.75 if dark else 0.96),
        "darker_background": mix(bg, 0.55 if dark else 0.92),
        "lighter_background": mix(bg, 1.7 if dark else 0.97),
        "foreground": fg,
        "dark_foreground": bright.get("black") or mix(fg, 0.65),
        "light_foreground": mix(fg, 1.08),
        "bright_foreground": bright.get("white") or mix(fg, 1.15),
        "red": normal.get("red"), "yellow": normal.get("yellow"),
        "green": normal.get("green"), "cyan": normal.get("cyan"),
        "blue": normal.get("blue"), "magenta": normal.get("magenta"),
        "bright_red": bright.get("red"), "bright_yellow": bright.get("yellow"),
        "bright_green": bright.get("green"), "bright_cyan": bright.get("cyan"),
        "bright_blue": bright.get("blue"), "bright_magenta": bright.get("magenta"),
    }
    return {k: v for k, v in colors.items() if v and HEX.match(v)}, dark


def from_indexed(raw, dark_hint=None):
    """kitty-style color0..color15, as some themes ship instead of roles."""
    idx = {f"color{i}": raw.get(f"color{i}") for i in range(16)}
    bg, fg = raw.get("background"), raw.get("foreground")
    if not bg or not fg or not idx.get("color1"):
        return None
    dark = dark_hint if dark_hint is not None else luminance(bg) <= 0.5
    colors = {
        "accent": raw.get("accent") or raw.get("cursor") or idx["color4"],
        "selection": raw.get("selection_background") or mix(bg, 1.8 if dark else 0.92),
        "muted": idx.get("color8") or mix(bg, 2.4 if dark else 0.8),
        "background": bg,
        "dark_background": mix(bg, 0.75 if dark else 0.96),
        "darker_background": mix(bg, 0.55 if dark else 0.92),
        "lighter_background": mix(bg, 1.7 if dark else 0.97),
        "foreground": fg,
        "dark_foreground": idx.get("color8") or mix(fg, 0.65),
        "light_foreground": mix(fg, 1.08),
        "bright_foreground": idx.get("color15") or mix(fg, 1.15),
        "red": idx.get("color1"), "green": idx.get("color2"),
        "yellow": idx.get("color3"), "blue": idx.get("color4"),
        "magenta": idx.get("color5"), "cyan": idx.get("color6"),
        "bright_red": idx.get("color9"), "bright_green": idx.get("color10"),
        "bright_yellow": idx.get("color11"), "bright_blue": idx.get("color12"),
        "bright_magenta": idx.get("color13"), "bright_cyan": idx.get("color14"),
    }
    return {k: v for k, v in colors.items() if v and HEX.match(v)}, dark


def from_colors_toml(path):
    raw = parse_pairs(open(path, encoding="utf-8", errors="replace").read())
    colors = {k: v for k, v in raw.items() if k in WANTED and HEX.match(v)}
    if "background" not in colors:
        return None
    text = open(path, encoding="utf-8", errors="replace").read()
    m = re.search(r'mode\s*=\s*"(\w+)"', text)
    dark = (m.group(1) == "dark") if m else luminance(colors["background"]) <= 0.5
    return colors, dark


def label_for(theme_id):
    special = {"retropc": "RetroPC", "synthwave84": "Synthwave '84",
               "azure-glow": "Azure Glow", "waveform-dark": "Waveform Dark",
               "neon-dusk": "Neon Dusk", "retro-bloom": "Retro Bloom",
               "space-monkey": "Space Monkey", "blackgold": "Black Gold",
               "moodpeak": "Moodpeak", "archwave": "Archwave"}
    if theme_id in special:
        return special[theme_id]
    return " ".join(w.capitalize() for w in theme_id.replace("-", " ").split())


def main(root, only=None):
    out = []
    for theme_id in sorted(os.listdir(root)):
        if only and theme_id not in only:
            continue
        d = os.path.join(root, theme_id)
        if not os.path.isdir(d):
            continue
        ct = os.path.join(d, "colors.toml")
        al = os.path.join(d, "alacritty.toml")
        got = None
        if os.path.isfile(ct):
            got = from_colors_toml(ct)
            # A colors.toml that is missing required roles still needs the
            # terminal fallback to fill them in.
            if got and not all(k in got[0] for k in REQUIRED):
                raw = parse_pairs(open(ct, encoding="utf-8", errors="replace").read())
                alt = from_indexed(raw, got[1])
                if alt is None and os.path.isfile(al):
                    alt = from_alacritty(al)
                if alt:
                    merged = dict(alt[0]); merged.update(got[0])
                    got = (merged, got[1])
        elif os.path.isfile(al):
            got = from_alacritty(al)
        if not got:
            print(f"SKIP {theme_id}: no usable palette", file=sys.stderr)
            continue
        colors, dark = got
        missing = [k for k in REQUIRED if k not in colors]
        if missing:
            print(f"SKIP {theme_id}: missing {','.join(missing)}", file=sys.stderr)
            continue
        out.append({"id": theme_id, "label": label_for(theme_id),
                    "mode": "dark" if dark else "light",
                    "colors": {k: colors[k] for k in WANTED if k in colors}})
    json.dump(out, sys.stdout, indent=2)


if __name__ == "__main__":
    main(sys.argv[1], set(sys.argv[2:]) or None)
