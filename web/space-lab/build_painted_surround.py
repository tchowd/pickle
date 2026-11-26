"""Register the painted outpainting to the existing 16:9 scene and trace the margins.

Usage: python build_painted_surround.py <portrait painting> <tall painting>
The second painting supplies additional roof timbers and floor for narrow screens.
"""
import argparse
import io
from pathlib import Path
from xml.etree import ElementTree

import numpy as np
from PIL import Image, ImageFilter
import vtracer

ROOT = Path(__file__).parent
WIDTH = 1024
# (x in scene, horizontal correction, y in painted reference). The painting's
# poster and shelf lids sit at different heights from the central star field.
UPPER = [(0,0,327),(47,1,327),(114,2,326),(145,0,286),(161,0,285),
         (199,1,282),(245,4,278),(283,8,278),(301,0,327),(850,0,327),
         (880,-1,321),(919,0,321),(938,0,317),(1010,0,317),(1023,0,317)]
LOWER = [(0,0,903),(480,0,903),(620,-1,903),(800,0,903),(1023,0,903)]


def registered_band(paint, reference, upper):
    anchors = np.array(UPPER if upper else LOWER)
    height = 327 if upper else 633
    y, x = np.mgrid[:height, :WIDTH]
    dx = np.interp(x[0], anchors[:, 0], anchors[:, 1])
    edge = np.interp(x[0], anchors[:, 0], anchors[:, 2])
    depth = (height - 1 - y) if upper else y
    influence = np.maximum(0, 1 - depth / 164) ** 2
    px = x + influence * dx
    py = y + (0 if upper else 903) + influence * (edge - (327 if upper else 903))
    # Bilinear sampling of the landmark warp.
    px, py = np.clip(px, 0, WIDTH - 1.001), np.clip(py, 0, 1534.999)
    ix, iy = px.astype(int), py.astype(int)
    tx, ty = (px - ix)[..., None], (py - iy)[..., None]
    rgb = ((1-ty)*((1-tx)*paint[iy, ix] + tx*paint[iy, ix+1]) +
           ty*((1-tx)*paint[iy+1, ix] + tx*paint[iy+1, ix+1]))
    # The freshly painted tabletop is warmer in the foreground. Correct its
    # broad color cast near the cut while preserving ink and local highlights.
    target = reference[:2] if upper else reference[-2:]
    painted = rgb[-2:] if upper else rgb[:2]
    def blurred_row(strip, radius):
        row = Image.fromarray(strip.mean(axis=0, keepdims=True).astype('uint8'))
        return np.asarray(row.filter(ImageFilter.GaussianBlur(radius)), dtype=float)
    broad = blurred_row(target, 6) - blurred_row(painted, 6)
    local = blurred_row(target, 0.4) - blurred_row(painted, 0.4)
    rgb += broad * np.exp(-depth[..., None] / 53)
    rgb += (local - broad) * np.exp(-depth[..., None] / 5)
    return Image.fromarray(np.clip(rgb, 0, 255).astype('uint8'))


def extend_band(near, tall, upper):
    # Match the ceiling insulators and cabinet feet between the two paintings.
    # The outer painting has a slight horizontal drift from left to right.
    extra = 300 if upper else 479
    height = near.height + extra
    y, x = np.mgrid[:height, :WIDTH]
    source_y = (y if upper else y + 903 + 289)
    dx = np.interp(x[0], [0, 300, 640, 1023], [1, 2, -2, -1] if upper else [4, 2, 0, -2])
    far = tall[np.clip(source_y, 0, tall.shape[0]-1),
               np.clip(np.rint(x + dx).astype(int), 0, WIDTH-1)]
    nearest = np.asarray(near)[np.clip(y - (extra if upper else 0), 0, near.height-1), x]
    mix = np.clip((y - 372) / 20 if upper else (531 - y) / 20, 0, 1)[..., None]
    return Image.fromarray((far * (1-mix) + nearest * mix).astype('uint8'))


def trace_section(image, scale):
    data = io.BytesIO()
    image.resize((round(image.width * scale), round(image.height * scale)),
                 Image.Resampling.LANCZOS).save(data, format='PNG')
    svg = vtracer.convert_raw_image_to_svg(
        data.getvalue(), 'png', colormode='color', hierarchical='stacked', mode='spline',
        filter_speckle=2, color_precision=7, layer_difference=10, length_threshold=3,
        path_precision=1)
    return list(ElementTree.fromstring(svg))


def write_vectors(image, name, destination, upper):
    ns = '{http://www.w3.org/2000/svg}'
    doc = ElementTree.Element(ns + 'svg')
    doc.set('viewBox', f'0 0 {image.width} {image.height}')
    group = ElementTree.SubElement(doc, ns + 'g', id=name)
    # Reserve the finest trace for cuts through small lettering, papers and cables.
    split = image.height - 100 if upper else 100
    sections = [(0, split, 1 if upper else 2), (split, image.height, 2 if upper else 1)]
    for start, end, scale in sections:
        details = ElementTree.SubElement(group, ns + 'g', transform=f'translate(0 {start}) scale({1/scale:g})')
        details.extend(trace_section(image.crop((0, start, image.width, end)), scale))
    doc.set('width', str(image.width))
    doc.set('height', str(image.height))
    ElementTree.register_namespace('', ns[1:-1])
    ElementTree.ElementTree(doc).write(destination, encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('painting', type=Path)
    parser.add_argument('tall_painting', type=Path)
    args = parser.parse_args()
    with Image.open(args.painting) as source:
        paint = np.asarray(source.convert('RGB').resize((1024, 1536)), dtype=float)
    with Image.open(ROOT / 'workbench.png') as source:
        ref = np.asarray(source.convert('RGB').resize((1024, 576), Image.Resampling.LANCZOS))
    with Image.open(args.tall_painting) as source:
        tall = np.asarray(source.convert('RGB').resize((1024, 2304), Image.Resampling.LANCZOS))
    for upper, name in [(True, 'rafters'), (False, 'bench-cabinet')]:
        band = extend_band(registered_band(paint, ref, upper), tall, upper)
        write_vectors(band, name, ROOT / 'src/artwork' / f'{name}.svg', upper)
