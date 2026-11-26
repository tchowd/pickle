"""Build the editable SVG scene from a supplied reference (Pillow and vtracer)."""
import argparse
import io
import json
from pathlib import Path
from xml.etree import ElementTree as ET

from PIL import Image, ImageDraw, ImageFilter
import vtracer


# Silhouettes in the reference's 2048 × 1152 composition, back to front.
# Each mask claims only the visible part; nearer objects own their overlaps.
OBJECTS = {
    "star-wall": [(0,0),(2048,0),(2048,1152),(0,1152)],
    "window-and-curtain": [(0,0),(228,0),(235,575),(192,611),(0,609)],
    "space-fleet-poster": [(321,0),(565,0),(563,315),(322,310)],
    "shelves-and-curios": [(1533,165),(1563,157),(1569,91),(1737,40),(1745,0),(2048,0),(2048,795),(1544,767)],
    "wooden-workbench-and-wiring": [(0,703),(339,681),(751,639),(1195,663),(1568,638),(2048,730),(2048,1152),(0,1152)],
    "model-rockets": [(211,687),(215,418),(236,393),(242,419),(247,390),(267,347),(285,385),(291,411),(309,503),(311,572),(323,567),(323,490),(339,479),(350,493),(350,450),(374,407),(398,444),(406,545),(427,545),(432,494),(445,468),(459,500),(465,393),(485,343),(505,396),(514,544),(537,514),(546,331),(560,303),(571,332),(572,273),(594,232),(611,267),(615,328),(628,302),(642,333),(641,510),(658,544),(649,571),(589,586),(534,611),(342,681)],
    "wall-notes": [(656,369),(692,369),(694,354),(704,354),(703,368),(739,369),(731,465),(651,459)],
    "warning-sign": [(1414,215),(1524,214),(1525,378),(1412,377)],
    "anglepoise-lamp": [(92,735),(179,707),(140,545),(103,408),(99,368),(115,348),(134,348),(248,188),(242,174),(252,150),(271,139),(291,119),(270,119),(278,101),(311,88),(338,103),(371,103),(416,182),(453,191),(491,212),(530,247),(546,279),(545,302),(517,331),(477,358),(418,384),(354,398),(314,397),(293,382),(292,343),(293,302),(304,258),(321,220),(299,192),(278,186),(154,377),(144,403),(211,707),(227,726),(258,743),(255,780),(113,800)],
    "specimen-jar-left": [(0,478),(25,476),(37,539),(100,539),(120,550),(123,593),(139,620),(148,758),(124,794),(0,800)],
    "oscilloscope": [(370,594),(394,585),(690,574),(728,589),(738,768),(713,791),(387,803),(370,780)],
    "computer-base": [(774,720),(807,657),(1304,657),(1328,721),(1325,814),(1308,834),(797,832),(775,816)],
    "crt-housing": [(751,622),(758,217),(764,185),(779,172),(830,167),(1109,165),(1329,174),(1348,187),(1355,230),(1355,603),(1347,652),(1332,668),(1217,672),(1157,687),(841,684),(769,656)],
    "portal-glass": [(824,249),(837,237),(884,232),(994,229),(1173,231),(1269,239),(1276,250),(1281,335),(1284,464),(1276,558),(1269,572),(1215,578),(1050,581),(885,578),(829,569),(820,558),(816,486),(818,335)],
    "dimension-note": [(1223,599),(1316,595),(1325,684),(1230,693)],
    "specimen-chamber": [(1367,687),(1387,648),(1434,631),(1437,426),(1446,412),(1465,408),(1470,397),(1526,395),(1538,409),(1559,417),(1564,435),(1559,631),(1586,643),(1609,665),(1608,753),(1571,781),(1420,784),(1379,759)],
    "time-controller": [(1593,810),(1606,667),(1618,644),(1631,637),(1632,609),(1715,597),(1792,607),(1807,626),(1854,630),(1884,651),(1882,807),(1860,848),(1812,877),(1624,852)],
    "puzzle-cube": [(1892,637),(1963,619),(2017,631),(2020,707),(1983,730),(1896,715)],
    "soldering-iron": [(0,810),(70,786),(108,790),(184,730),(235,708),(261,708),(281,722),(255,742),(157,827),(122,874),(146,891),(201,892),(215,908),(205,949),(164,971),(0,976)],
    "pencil-mug": [(260,749),(274,738),(259,706),(267,693),(283,705),(298,743),(301,721),(317,713),(332,744),(354,744),(353,715),(367,710),(367,740),(381,720),(393,727),(390,742),(415,741),(427,762),(426,812),(409,833),(402,856),(371,869),(281,867),(269,856)],
    "notebook": [(216,908),(233,885),(481,834),(500,838),(635,1005),(625,1032),(574,1054),(273,1133),(250,1118)],
    "keyboard": [(1001,930),(1010,896),(1137,819),(1155,814),(1776,919),(1802,933),(1805,956),(1726,1099),(1709,1120),(1686,1116),(1020,959)],
    "digital-meter": [(746,938),(765,910),(845,888),(904,906),(916,944),(894,981),(842,992),(756,968)],
    "handheld-meter": [(529,1101),(586,1019),(611,984),(685,994),(722,1014),(718,1042),(627,1147),(558,1131)],
    "screwdriver": [(1012,1132),(1029,1122),(1169,1083),(1176,1066),(1201,1056),(1310,1039),(1357,1051),(1350,1081),(1235,1111),(1190,1115),(1181,1102)],
    "calculator": [(1830,938),(1926,853),(1971,843),(2048,869),(2048,926),(1949,1005),(1843,981)],
    "mouse": [(1833,1058),(1842,1039),(1879,1014),(1916,1019),(1961,1037),(1996,1080),(1991,1114),(1969,1126),(1930,1125),(1868,1098)],
}


def build(reference, output):
    size = (2048, 1152)
    source = Image.open(reference).convert("RGB").resize(size, Image.Resampling.LANCZOS)
    ownership = Image.new("L", size)
    draw = ImageDraw.Draw(ownership)
    for index, polygon in enumerate(OBJECTS.values()):
        draw.polygon(polygon, fill=index)

    output.mkdir(parents=True, exist_ok=True)
    for index, name in enumerate(OBJECTS):
        # Extend underlying paint two pixels to prevent antialiasing seams.
        mask = ownership.point(lambda value: 255 if value == index else 0)
        mask = mask.filter(ImageFilter.MaxFilter(5))
        bounds = mask.getbbox()
        region = source.crop(bounds)
        region.putalpha(mask.crop(bounds))
        encoded = io.BytesIO()
        region.save(encoded, format="PNG")
        curves = vtracer.convert_raw_image_to_svg(
            encoded.getvalue(), "png", colormode="color", hierarchical="stacked",
            mode="spline", filter_speckle=2, color_precision=7, layer_difference=12,
            length_threshold=3.5, path_precision=1)
        geometry = ET.fromstring(curves)
        parts = [f'<svg xmlns="http://www.w3.org/2000/svg"><g transform="translate({bounds[0]} {bounds[1]})">']
        for path in geometry:
            attrs = " ".join(f'{key}="{value}"' for key, value in path.attrib.items())
            parts.append(f'<path {attrs}/>')
        parts.append('</g></svg>')
        (output / f"{name}.svg").write_text("\n".join(parts) + "\n")
    (output / "layers.json").write_text(json.dumps(list(OBJECTS), indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    build(args.reference, args.output)
