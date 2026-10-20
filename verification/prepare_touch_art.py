"""Package supplied art without repainting it or changing control hit geometry.

Requirements: Pillow. Run from any directory; original uploads stay in artwork-source.
"""
from pathlib import Path
import json
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'artwork-source/touch-update'
ASSETS = ROOT / 'android/app/src/main/assets'
report = {}
for name in ['v', 'z', 'c', 'x', 'joystick-base', 'joystick-knob', 'alpha8']:
    im = Image.open(SOURCE / (name + '.png')).convert('RGBA')
    # Uploaded PNGs have nearly invisible alpha specks in their outer margin.
    box = im.getchannel('A').point(lambda a: 255 if a >= 8 else 0).getbbox()
    assert box, name
    crop = im.crop(box)
    # The original knob occupied 24/50 of its canvas. Keep its visible size.
    fit = 115 if name == 'joystick-knob' else 240
    scale = fit / max(crop.size)
    size = tuple(max(1, round(v * scale)) for v in crop.size)
    crop = crop.resize(size, Image.Resampling.NEAREST)
    out = Image.new('RGBA', (240, 240))
    out.paste(crop, ((240 - size[0]) // 2, (240 - size[1]) // 2))
    out.save(ASSETS / (name + '.png'), optimize=True)
    report[name] = dict(source_size=im.size, crop=box, useful_size=size, canvas=out.size)
(ROOT / 'verification/touch-art.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
