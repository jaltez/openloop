#!/usr/bin/env python3
"""Convert `tmux capture-pane -e -p` output into a faithful terminal
screenshot SVG (per-cell glyphs, so alignment is font-metric independent).

Usage: ansi2svg.py <capture.txt> <out.svg> <window-title> [cols rows]"""
import html, sys

COLS, ROWS = 112, 34
CW, CH = 8.4, 17.0          # cell size
PAD = 18                    # inner padding
TITLE_H = 36                # window title bar
FS = 13.5                   # font size
CANVAS_BG = "#0f0f1a"
DEFAULT_FG = "#e2e8f0"

NAMED_FG = {  # 30-37 / 90-97 fallbacks (theme itself is truecolor)
    "30": "#3b4252", "31": "#bf616a", "32": "#a3be8c", "33": "#ebcb8b",
    "34": "#81a1c1", "35": "#b48ead", "36": "#88c0d0", "37": "#e5e9f0",
    "90": "#4c566a", "91": "#d08770", "92": "#9ece6a", "93": "#e0af68",
    "94": "#7aa2f7", "95": "#bb9af7", "96": "#7dcfff", "97": "#c0caf5",
}

def parse_sgr(params, state):
    i = 0
    while i < len(params):
        p = params[i]
        if p == 0:
            state.update(fg=None, bg=None, bold=False)
        elif p == 1:
            state["bold"] = True
        elif p == 22:
            state["bold"] = False
        elif p in (39, 49):
            state["fg" if p == 39 else "bg"] = None
        elif p in (38, 48):
            key = "fg" if p == 38 else "bg"
            if i + 1 < len(params) and params[i + 1] == 2:      # 38;2;r;g;b
                state[key] = "#%02x%02x%02x" % tuple(params[i + 2:i + 5])
                i += 4
            elif i + 1 < len(params) and params[i + 1] == 5:    # 38;5;n
                n = params[i + 2]
                if n < 16:
                    state[key] = NAMED_FG[str(30 + n % 8 if n < 8 else 90 + n % 8)]
                else:  # xterm 256 cube approximation (never hit by this theme)
                    state[key] = "#%02x%02x%02x" % (
                        (n - 16) // 36 * 51, ((n - 16) // 6 % 6) * 51, (n - 16) % 6 * 51)
                i += 2
            else:
                break
        elif 30 <= p <= 37 or 90 <= p <= 97:
            state["fg"] = NAMED_FG[str(p)]
        i += 1

def parse_lines(text, cols):
    """-> list of rows; row = list of (char, fg, bg, bold) cells."""
    grid = []
    for raw in text.split("\n"):
        if raw.endswith("\r"):
            raw = raw[:-1]
        state = {"fg": None, "bg": None, "bold": False}
        cells = []
        i = 0
        while i < len(raw):
            if raw[i] == "\x1b" and i + 1 < len(raw) and raw[i + 1] == "[":
                j = raw.find("m", i + 2)
                if j == -1:
                    break
                params = [int(x) if x else 0 for x in raw[i + 2:j].split(";")]
                parse_sgr(params, state)
                i = j + 1
            else:
                cells.append((raw[i], state["fg"], state["bg"], state["bold"]))
                i += 1
        # extend trailing panel background to full width when tmux trimmed it
        last_bg = next((c[2] for c in reversed(cells) if c[2]), None)
        while len(cells) < cols and last_bg:
            cells.append((" ", None, last_bg, False))
        grid.append(cells)
    return grid

def render(grid, out_path, title, cols):
    rows = len(grid)
    W = PAD * 2 + cols * CW
    H = TITLE_H + PAD + rows * CH + PAD
    s = []
    s.append(f'<svg xmlns="http://www.w3.org/2000/svg" width="{W:.0f}" height="{H:.0f}" '
             f'viewBox="0 0 {W:.0f} {H:.0f}" '
             f'font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace">')
    s.append(f'<rect x="1" y="1" width="{W-2:.0f}" height="{H-2:.0f}" rx="10" '
             f'fill="#0b0b13" stroke="#32324a" stroke-width="1.5"/>')
    s.append(f'<path d="M 1 11 a 10 10 0 0 1 10 -10 h {W-22:.0f} '
             f'a 10 10 0 0 1 10 10 v {TITLE_H-11:.0f} h -{W-2:.0f} z" fill="#16161f"/>')
    for cx, col in ((22, "#ff5f57"), (42, "#febc2e"), (62, "#28c840")):
        s.append(f'<circle cx="{cx}" cy="{TITLE_H/2:.0f}" r="6" fill="{col}"/>')
    s.append(f'<text x="{W/2:.0f}" y="{TITLE_H/2+4.5:.0f}" font-size="12" fill="#6b7394" '
             f'text-anchor="middle">{html.escape(title)}</text>')
    s.append(f'<rect x="{PAD}" y="{TITLE_H+PAD}" width="{cols*CW:.0f}" '
             f'height="{rows*CH:.0f}" fill="{CANVAS_BG}"/>')
    top = TITLE_H + PAD
    for r, row in enumerate(grid):
        y = top + r * CH + CH * 0.5 + FS * 0.36  # baseline
        c = 0
        while c < len(row):  # merged background runs
            bg = row[c][2]
            if bg and bg != CANVAS_BG:
                c2 = c
                while c2 < len(row) and row[c2][2] == bg:
                    c2 += 1
                s.append(f'<rect x="{PAD+c*CW:.1f}" y="{top+r*CH:.1f}" '
                         f'width="{(c2-c)*CW:.1f}" height="{CH:.1f}" fill="{bg}"/>')
                c = c2
            else:
                c += 1
        for c, (ch, fg, bg, bold) in enumerate(row):
            if ch == " ":
                continue
            style = f'fill="{fg or DEFAULT_FG}"'
            if bold:
                style += ' font-weight="700"'
            s.append(f'<text x="{PAD+(c+0.5)*CW:.1f}" y="{y:.1f}" font-size="{FS}" '
                     f'text-anchor="middle" {style}>{html.escape(ch)}</text>')
    s.append("</svg>")
    open(out_path, "w").write("\n".join(s))
    print(f"wrote {out_path} ({rows} rows)")

if __name__ == "__main__":
    src, dst, title = sys.argv[1], sys.argv[2], sys.argv[3]
    cols = int(sys.argv[4]) if len(sys.argv) > 4 else COLS
    grid = parse_lines(open(src, encoding="utf-8").read(), cols)
    while len(grid) > 1 and not any(ch != " " for ch, *_ in grid[-1]):
        grid.pop()  # tmux trailing-newline artifact
    render(grid, dst, title, cols)
