#!/usr/bin/env python3
"""Resolve actual design tokens; check text contrast; write an SVG review sheet."""
import json
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


def luminance(color):
    rgb = [int(color[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    rgb = [v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4 for v in rgb]
    return sum(v * w for v, w in zip(rgb, (0.2126, 0.7152, 0.0722)))


def contrast(a, b):
    light, dark = sorted((luminance(a), luminance(b)), reverse=True)
    return (light + 0.05) / (dark + 0.05)


def main():
    palettes = json.loads((ROOT / 'design/palettes.json').read_text())
    tokens = json.loads((ROOT / 'design/tokens.json').read_text())
    reports = []
    svg = ['<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="830" viewBox="0 0 1200 830">',
           '<title>UltraTasks draft design tokens</title>',
           '<desc>Dark and light semantic colors with actual Radix colors and computed text contrast. This is a design review sheet, not an application screenshot.</desc>',
           '<rect width="1200" height="830" fill="#e8e8ec"/>',
           '<g font-family="Inter, Segoe UI, sans-serif">',
           '<text x="36" y="42" font-size="22" fill="#1c2024">UltraTasks / Color foundation</text>',
           '<text x="36" y="68" font-size="13" fill="#60646c">Radix Colors · draft semantic mapping · native UI validation pending</text>']
    for mode, x in [('dark', 24), ('light', 612)]:
        p = palettes[mode]
        t = {key: p[ref.split('.')[0]][ref.split('.')[1]] for key, ref in tokens['themes'][mode].items()}
        pairs = [('text_primary', 'canvas'), ('text_secondary', 'canvas'),
                 ('text_primary', 'surface'), ('text_secondary', 'surface'),
                 ('text_primary', 'surface_hover'), ('text_secondary', 'surface_hover'),
                 ('action_text', 'action_background'), ('selection_text', 'selection_background'),
                 ('positive_text', 'positive_background'), ('warning_text', 'warning_background'),
                 ('error_text', 'error_background')]
        for fg, bg in pairs:
            ratio = contrast(t[fg], t[bg])
            reports.append({'theme': mode, 'foreground': fg, 'background': bg, 'ratio': round(ratio, 3), 'minimum': 4.5, 'pass': ratio >= 4.5})
        svg.extend([f'<rect x="{x}" y="96" width="564" height="710" rx="12" fill="{t["canvas"]}"/>',
                    f'<text x="{x+28}" y="140" font-size="20" fill="{t["text_primary"]}">{mode.capitalize()} / Quiet focus</text>',
                    f'<text x="{x+28}" y="168" font-size="13" fill="{t["text_secondary"]}">Shared roles, distinct luminance</text>'])
        for i, name in enumerate(['slate', 'blue', 'jade', 'amber', 'tomato']):
            y = 202 + i * 70
            svg.append(f'<text x="{x+28}" y="{y}" font-size="12" fill="{t["text_secondary"]}">{name.upper()}</text>')
            for step in range(1, 13):
                color = p[name][f'{name}{step}']
                svg.append(f'<rect x="{x+28+(step-1)*42}" y="{y+12}" width="38" height="26" rx="4" fill="{color}"/>')
        svg.extend([f'<rect x="{x+28}" y="574" width="508" height="66" rx="6" fill="{t["surface"]}"/>',
                    f'<text x="{x+46}" y="601" font-size="14" fill="{t["text_primary"]}">Refine calendar interactions</text>',
                    f'<text x="{x+46}" y="624" font-size="12" fill="{t["text_secondary"]}">Today · 45 minutes · Work</text>',
                    f'<rect x="{x+28}" y="660" width="140" height="36" rx="6" fill="{t["action_background"]}"/>',
                    f'<text x="{x+98}" y="683" text-anchor="middle" font-size="13" fill="{t["action_text"]}">Add task</text>',
                    f'<rect x="{x+184}" y="660" width="352" height="36" rx="6" fill="{t["selection_background"]}"/>',
                    f'<text x="{x+200}" y="683" font-size="13" fill="{t["selection_text"]}">Task scheduled · Undo</text>',
                    f'<text x="{x+28}" y="742" font-size="12" fill="{t["text_secondary"]}">Body 14 / 20 · Desktop row 36 · Android target 48dp</text>',
                    f'<text x="{x+28}" y="770" font-size="12" fill="{t["text_secondary"]}">Text pair checks recorded in contrast-report.json</text>'])
    svg.append('</g></svg>')
    (ROOT / 'design/palette-preview.svg').write_text('\n'.join(svg) + '\n')
    (ROOT / 'design/contrast-report.json').write_text(json.dumps({'method': 'WCAG relative luminance, opaque sRGB, selected normal-text pairs only; not full accessibility certification', 'results': reports}, indent=2) + '\n')
    for file in (ROOT / 'design/icons').glob('*.svg'):
        ET.parse(file)
    ET.parse(ROOT / 'design/palette-preview.svg')
    failures = [r for r in reports if not r['pass']]
    print(f'{len(reports)} text contrast pairs checked; {len(failures)} below 4.5:1. SVG syntax verified.')
    if failures:
        print(json.dumps(failures, indent=2))
        raise SystemExit(1)


if __name__ == '__main__':
    main()
