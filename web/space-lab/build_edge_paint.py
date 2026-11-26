"""Trace material continuations at the two cut edges. Requires Pillow and vtracer."""
import io
import math
import random
from pathlib import Path
from xml.etree import ElementTree as ET

from PIL import Image
import vtracer


ROOT = Path(__file__).parent
WIDTH = 2048


def trace(image, x, y):
    encoded = io.BytesIO()
    image.save(encoded, format="PNG")
    svg = vtracer.convert_raw_image_to_svg(
        encoded.getvalue(), "png", colormode="color", hierarchical="stacked",
        mode="spline", filter_speckle=2, color_precision=7, layer_difference=12,
        length_threshold=3.5, path_precision=1)
    paths = []
    for path in ET.fromstring(svg):
        attrs = " ".join(f'{k}="{v}"' for k, v in path.attrib.items())
        paths.append(f"<path {attrs}/>")
    return f'<g transform="translate({x} {y})">' + "\n".join(paths) + "</g>"


def continue_strip(source, x0, x1, height, top, slope, sample_depth):
    # Keep the exact registration at the join. Sample a shallow strip along
    # the material's grain; slope continues the perspective of each object.
    patch = Image.new("RGB", (x1 - x0, height))
    for row in range(height):
        depth = height - 1 - row if top else row
        for col in range(patch.width):
            x = x0 + col
            drift = depth * slope + math.sin(depth / 28 + x / 70) * min(depth / 10, 2)
            sx = min(WIDTH - 1, max(0, round(x - drift)))
            wave = depth * (.44 + .16 * math.sin(x / 59)) / sample_depth
            sy = round(sample_depth * abs(math.sin(wave)))
            color = source.getpixel((sx, sy if top else 1151 - sy))
            patch.putpixel((col, row), color)
    return trace(patch, x0, -height if top else 1152)


def main():
    source = Image.open(ROOT / "workbench.png").convert("RGB").resize(
        (WIDTH, 1152), Image.Resampling.LANCZOS)
    parts = ['<svg xmlns="http://www.w3.org/2000/svg">']
    for name, x0, x1, h, top, slope, depth in [
        ("curtain-continuation", 0, 232, 240, True, -.035, 13),
        ("poster-continuation", 318, 567, 24, True, .03, 2),
        ("shelf-continuation", 1725, 2048, 23, True, .02, 2),
        ("desk-continuation", 0, 2048, 65, False, -.32, 6),
    ]:
        parts.append(f'<g id="{name}">{continue_strip(source, x0, x1, h, top, slope, depth)}</g>')
    # Use the desk's local dark-to-light palette for long, nonparallel fibres.
    # The broad grain and short scraped highlights vary independently.
    rng = random.Random(381)
    def field(w, h):
        plane = Image.new("L", (w, h))
        plane.putdata(rng.choices(range(256), k=w*h))
        return plane.resize((1024, 128), Image.Resampling.BICUBIC)
    broad, vein, pits = field(7, 18), field(28, 109), field(608, 124)
    plank = Image.new("RGB", (1024, 128))
    for y in range(128):
        for x in range(1024):
            shift = round(2.9 * math.sin(x / 92 + y / 21) + 1.6 * math.sin(x / 177))
            v = vein.getpixel((x, (y + shift) % 128)) / 255
            tone = .36 * broad.getpixel((x, y)) / 255 + .46 * v + .18 * pits.getpixel((x, y)) / 255
            plank.putpixel((x, y), (int(17 + tone * 105), int(10 + tone * 60), int(6 + tone * 26)))
    parts.append('<g id="wood-swatch">' + trace(plank, 0, 0) + '</g>')
    for i, box in enumerate([(614, 29, 652, 72), (932, 8, 975, 47), (1150, 46, 1194, 89)]):
        parts.append(f'<g id="painted-star-{i}">' + trace(source.crop(box), -21, -21) + '</g>')
    parts.append('</svg>')
    (ROOT / 'src/artwork/surround-materials.svg').write_text("\n".join(parts))


if __name__ == '__main__':
    main()
