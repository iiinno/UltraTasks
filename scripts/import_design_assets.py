#!/usr/bin/env python3
"""Import selected OSS assets at recorded commits. Requires authenticated gh."""
import base64
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / 'design' / 'upstream-lock.json'


def api(path):
    return json.loads(subprocess.check_output(['gh', 'api', path], text=True))


def content(repo, sha, path):
    obj = api(f'repos/{repo}/contents/{path}?ref={sha}')
    return base64.b64decode(obj['content']).decode('utf-8')


def write(path, text):
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, encoding='utf-8')


def main():
    lock = json.loads(LOCK.read_text()) if LOCK.exists() else {}
    for repo in ['radix-ui/colors', 'lucide-icons/lucide']:
        if repo not in lock:
            lock[repo] = {'commit': api(f'repos/{repo}/commits/main')['sha']}
    palettes = {}
    repo = 'radix-ui/colors'
    sha = lock[repo]['commit']
    for mode in ['light', 'dark']:
        source = content(repo, sha, f'src/{mode}.ts')
        palettes[mode] = {}
        for name in ['slate', 'blue', 'jade', 'amber', 'tomato']:
            export_name = name + ('Dark' if mode == 'dark' else '')
            match = re.search(r'export const ' + export_name + r' = \{(.*?)\};', source, re.S)
            if not match:
                raise ValueError(f'Missing {mode} {name}; upstream shape changed')
            values = dict(re.findall(r'(\w+): "(#[0-9a-fA-F]{6})"', match[1]))
            if len(values) != 12:
                raise ValueError(f'Expected 12 {mode} {name} steps')
            palettes[mode][name] = values
    write('design/palettes.json', json.dumps(palettes, indent=2) + '\n')
    write('third_party/radix-colors/LICENSE', content(repo, sha, 'LICENSE'))
    lock[repo].update({'license': 'MIT', 'upstream_files': ['src/light.ts', 'src/dark.ts', 'LICENSE'], 'local_files': ['design/palettes.json', 'third_party/radix-colors/LICENSE'], 'change': 'Extracted slate, blue, jade, amber, tomato hex scales to JSON; values unchanged.'})
    repo = 'lucide-icons/lucide'
    sha = lock[repo]['commit']
    icons = ['calendar-days', 'circle-check', 'inbox', 'list-todo', 'plus', 'search', 'clock', 'link', 'undo-2', 'chevron-left', 'chevron-right']
    for icon in icons:
        write(f'design/icons/{icon}.svg', content(repo, sha, f'icons/{icon}.svg'))
    write('third_party/lucide/LICENSE', content(repo, sha, 'LICENSE'))
    lock[repo].update({'license': 'ISC; includes MIT notice for Feather-derived icons', 'upstream_files': [f'icons/{icon}.svg' for icon in icons] + ['LICENSE'], 'local_files': [f'design/icons/{icon}.svg' for icon in icons] + ['third_party/lucide/LICENSE'], 'change': 'SVG files and complete license copied unchanged.'})
    write('design/upstream-lock.json', json.dumps(lock, indent=2) + '\n')
    for repo, data in lock.items():
        print(repo, data['commit'])
    print('Imported 120 color values and 11 SVG icons with full license notices.')


if __name__ == '__main__':
    main()
