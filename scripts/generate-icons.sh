#!/usr/bin/env bash
# Only needed when changing the SVG. Generated assets are committed.
set -euo pipefail
cd "$(dirname "$0")/.."
command -v magick >/dev/null || { echo 'Install ImageMagick to regenerate icons.' >&2; exit 1; }
python3 - <<'PY'
from pathlib import Path
import struct
import subprocess
import tempfile
import xml.etree.ElementTree as ET

icons = Path('crates/app/assets/app-icons')
source = ET.parse(icons / 'aether.svg')
root = source.getroot()
ns = {'svg': 'http://www.w3.org/2000/svg'}
rect = root.find('svg:rect', ns)
stops = root.findall('.//svg:stop', ns)
colors = [stop.attrib['stop-color'] for stop in stops]
x, y, width, height, radius = [float(rect.attrib[k]) for k in ('x', 'y', 'width', 'height', 'rx')]


def magick(*args):
    subprocess.run(['magick', *map(str, args)], check=True)


# Build the gradient explicitly: ImageMagick's built-in SVG renderer can omit
# SVG gradient fills on builds without librsvg. Geometry and colors come from
# the canonical SVG, while the outlined glyph needs no platform-specific font.
with tempfile.TemporaryDirectory(prefix='aether-icons-') as temp:
    temp = Path(temp)
    root.remove(root.find('svg:defs', ns))
    root.remove(rect)
    glyph = temp / 'glyph.svg'
    source.write(glyph, encoding='utf-8', xml_declaration=True)
    mask = temp / 'mask.png'
    magick('-size', '1024x1024', 'xc:none', '-fill', 'white', '-draw',
           f'roundrectangle {x},{y} {x+width},{y+height} {radius},{radius}', mask)
    background = temp / 'background.png'
    magick('-size', '1024x1024', '-define', f'gradient:vector={x},{y} {x+width},{y+height}',
           f'gradient:{colors[0]}-{colors[1]}', mask,
           '-alpha', 'off', '-compose', 'CopyOpacity', '-composite', background)
    master = temp / 'master.png'
    magick(background, '(', '-background', 'none', glyph, ')',
           '-compose', 'Over', '-composite', master)
    for size in (16, 32, 48, 64, 128, 256, 512, 1024):
        magick(master, '-resize', f'{size}x{size}', f'PNG32:{icons / str(size)}.png')

# PNG-backed ICNS works on macOS without iconutil and can be generated on Linux.
chunks = b''
for tag, size in [(b'icp4', 16), (b'icp5', 32), (b'icp6', 64), (b'ic07', 128),
                  (b'ic08', 256), (b'ic09', 512), (b'ic10', 1024)]:
    data = (icons / f'{size}.png').read_bytes()
    chunks += tag + struct.pack('>I', len(data) + 8) + data
(icons / 'Aether.icns').write_bytes(b'icns' + struct.pack('>I', len(chunks) + 8) + chunks)
PY
