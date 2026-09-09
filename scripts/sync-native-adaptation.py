#!/usr/bin/env python3
"""Refresh the proposed first-party QML copy; does not edit host defaults."""
import argparse, hashlib, json, pathlib, subprocess
parser = argparse.ArgumentParser()
parser.add_argument('omarchy', type=pathlib.Path)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
source = pathlib.Path(__file__).resolve().parent.parent
target = args.omarchy / 'shell/plugins/panels/plugin-workbench'
if not args.omarchy.is_absolute() or target.is_symlink() or not target.is_dir():
    parser.error('expected an absolute checkout containing the proposed native integration')
files = {}
for name in ('BarWidget.qml', 'Panel.qml', 'Navigation.js'):
    content = (source / name).read_text()
    files[name] = hashlib.sha256(content.encode()).hexdigest()
    content = content.replace('io.github.tcballard.plugin-workbench', 'omarchy.plugin-workbench')
    if name == 'BarWidget.qml':
        begin = content.index('  readonly property string helperPath: {')
        end = content.index('\n  }', begin) + len('\n  }')
        content = content[:begin] + '  readonly property string helperPath: "/usr/bin/omarchy-plugin-workbench"' + content[end:]
    output = target / name
    if output.is_symlink(): parser.error('refusing symlink target')
    if args.check:
        if not output.exists() or output.read_text() != content: parser.error(f'{name} adaptation drifted')
    else: output.write_text(content)
provenance = {'repository': 'https://github.com/tcballard/omarchy-plugin-workbench',
              'baseCommit': subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip(),
              'workingTreeModified': bool(subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip()),
              'sourceSha256': files, 'helperProtocol': 1}
if not args.check: (target / 'UPSTREAM.json').write_text(json.dumps(provenance, indent=2) + '\n')
print('Native adaptation matches source' if args.check else 'Native adaptation refreshed')
