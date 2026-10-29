"""A browsable review page for what ``extract`` recovered: ``previews/index.html`` and the pictures it shows.

Each page is rendered with Ghostscript and overlaid with a clickable box for every recovered image, piece of vector
artwork and text line, so it is clear where an asset came from; layers toggle on and off. Below are thumbnails of the
images and vector pieces (a vector PDF cannot be shown in an ``<img>``, so each is rendered to a small PNG), then the
fonts, colour profiles and notes. Everything is relative to the output directory and needs no server.
"""

import asyncio
import html
import json
import logging
import pathlib
import typing

from PIL import Image

from . import proc

logger = logging.getLogger(__name__)

THUMB_PX = 360  # the longest side of a vector or image thumbnail
_MAX_THUMB_DPI = 600
_MIN_THUMB_DPI = 36
_JOBS = 4

_STYLE = """
:root { color-scheme: light dark; --line: #8886; --img: #1a73e8; --vec: #e8420c;
  --txt: #1b9e4b; --box: #888; --blk: #8e44ad; --tbl: #00838f; }
body { font: 14px/1.4 system-ui, sans-serif; margin: 0 auto; max-width: 1200px; padding: 16px; }
h1, h2 { margin: 24px 0 8px; } .meta { opacity: .7 } code { font-size: 12px }
.layers label { margin-right: 14px; }
.page { position: relative; margin: 8px 0; max-width: 900px; border: 1px solid var(--line); }
.page img { display: block; width: 100%; } .ov { position: absolute; border: 1px solid; box-sizing: border-box; }
.ov:hover { background: #ffd54f55; } .ov.img { border-color: var(--img); } .ov.vec { border-color: var(--vec); }
.ov.txt { border-color: var(--txt); border-width: 0 0 1px; }
.ov.blk { border-color: var(--blk); border-style: dashed; } .ov.tbl { border-color: var(--tbl); border-width: 2px; }
.ov.box { border-color: var(--box); border-style: dotted; }
.readout { position: sticky; top: 0; background: Canvas; padding: 4px 0; z-index: 2; font: 12px monospace }
.hide-img .img, .hide-vec .vec, .hide-txt .txt, .hide-box .box, .hide-blk .blk, .hide-tbl .tbl { display: none; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 10px; }
.card { border: 1px solid var(--line); padding: 8px; border-radius: 4px; overflow-wrap: anywhere; }
.card:target { outline: 3px solid #ffb300; } .thumb { height: 150px; display: flex; align-items: center;
justify-content: center; background: repeating-conic-gradient(#8883 0 25%, #0000 0 50%) 0 0 / 16px 16px; }
.thumb img { max-width: 100%; max-height: 100%; } table { border-collapse: collapse } td, th { padding: 2px 10px 2px 0;
text-align: left; vertical-align: top } details { margin: 6px 0 }
""".strip()

_SCRIPT = """
for (const box of document.querySelectorAll('.layers input'))
  box.addEventListener('change', () => document.body.classList.toggle('hide-' + box.dataset.layer, !box.checked));
for (const layer of ['box', 'txt']) document.querySelector('.layers input[data-layer=' + layer + ']').click();
// Point under the mouse, in PDF points from the page's bottom-left: the numbers `proof-pdf crop --box` takes.
const readout = document.getElementById('readout');
for (const page of document.querySelectorAll('.page'))
  page.addEventListener('mousemove', (event) => {
    const r = page.getBoundingClientRect(), w = +page.dataset.w, h = +page.dataset.h;
    const x = +page.dataset.x0 + (event.clientX - r.left) / r.width * w;
    const y = +page.dataset.y1 - (event.clientY - r.top) / r.height * h;
    readout.textContent = 'page ' + page.dataset.page + ': x ' + x.toFixed(1) + ', y ' + y.toFixed(1) + ' pt';
  });
""".strip()


async def build(
    document: dict[str, typing.Any], pdf_path: pathlib.Path, out_dir: pathlib.Path, *, dpi: int, gs_bin: str
) -> tuple[str, list[str]]:
    """Write ``previews/`` under *out_dir*; return ``(index path relative to out_dir, problems)``.

    *document* is ``Extraction.to_dict()``. Raises :class:`proof_pdf.preview.GhostscriptError` when the pages cannot
    be rendered; a thumbnail that cannot be made is a problem in the list, and the page still builds without it.
    """
    from .. import preview  # noqa: PLC0415 -- preview imports from _internal; a module-level import would be a cycle

    folder = out_dir / "previews"
    pages = await preview.preview_file(
        pdf_path, folder, dpi=dpi, guides=False, safe_mm=None, gs_bin=gs_bin, timeout=900.0
    )
    problems: list[str] = []
    thumbs = await _thumbnails(document["assets"], out_dir, folder, gs_bin, problems)
    (folder / "index.html").write_text(_page(document, [p.name for p in pages], thumbs), encoding="utf-8")
    return "previews/index.html", problems


async def _thumbnails(
    assets: list[dict], out_dir: pathlib.Path, folder: pathlib.Path, gs_bin: str, problems: list[str]
) -> dict[int, str]:
    """Thumbnail PNGs for the images and vector pieces, by asset index; path relative to the previews folder."""
    found: dict[int, str] = {}
    slots = asyncio.Semaphore(_JOBS)

    async def one(index: int, asset: dict) -> None:
        source = out_dir / asset["file"]
        target = folder / asset["kind"] / f"{source.stem}.png"
        target.parent.mkdir(parents=True, exist_ok=True)
        try:
            if asset["kind"] == "image":
                await asyncio.to_thread(_image_thumbnail, source, target)
            else:
                async with slots:
                    await _vector_thumbnail(gs_bin, source, target, asset["placements"][0])
        except (OSError, ValueError, proc.ProcTimeoutError, RuntimeError) as exc:
            problems.append(f"preview of {asset['file']}: {exc}")
            return
        found[index] = f"{asset['kind']}/{target.name}"

    await asyncio.gather(*(one(i, a) for i, a in enumerate(assets) if a["kind"] in ("image", "vector")))
    return found


def _image_thumbnail(source: pathlib.Path, target: pathlib.Path) -> None:
    with Image.open(source) as opened:
        picture = opened.convert("RGBA" if "A" in opened.getbands() or opened.mode == "P" else "RGB")
    picture.thumbnail((THUMB_PX, THUMB_PX))
    picture.save(target)


async def _vector_thumbnail(gs_bin: str, source: pathlib.Path, target: pathlib.Path, placement: dict) -> None:
    longest = max(placement["width"], placement["height"], 1.0)
    dpi = max(_MIN_THUMB_DPI, min(_MAX_THUMB_DPI, round(THUMB_PX / longest * 72)))
    result = await proc.run(
        [
            gs_bin,
            "-q",
            "-dNOPAUSE",
            "-dBATCH",
            "-dSAFER",
            "-sDEVICE=png16m",
            f"-r{dpi}",
            f"-sOutputFile={target}",
            str(source),
        ],
        timeout=120.0,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "ghostscript failed")


def _page(document: dict, page_pngs: list[str], thumbs: dict[int, str]) -> str:
    e = html.escape
    assets: list[dict] = document["assets"]
    parts = [
        f"<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>"
        f"<title>{e(pathlib.Path(document['path']).name)} - recovered assets</title><style>{_STYLE}</style>",
        f"<h1>{e(pathlib.Path(document['path']).name)}</h1><p class=meta>{len(document['pages'])} page(s), "
        f"{sum(a['kind'] == 'image' for a in assets)} image(s), {sum(a['kind'] == 'vector' for a in assets)} vector "
        f"piece(s), {len(document['lines'])} text line(s). Boxes on the pages link to the asset below.</p>",
        "<p class=layers><label><input type=checkbox data-layer=img checked> images</label>"
        "<label><input type=checkbox data-layer=vec checked> vector</label>"
        "<label><input type=checkbox data-layer=tbl checked> tables</label>"
        "<label><input type=checkbox data-layer=blk checked> text blocks</label>"
        "<label><input type=checkbox data-layer=txt checked> text lines</label>"
        "<label><input type=checkbox data-layer=box checked> rules and panels</label></p>",
        "<div id=readout class=readout>move over a page to read its coordinates (points, from bottom-left)</div>",
    ]
    parts.append(_summary(document["layout"]["summary"]))
    for note in document.get("notes", []):
        parts.append(f"<p><strong>Note:</strong> {e(note)}</p>")
    for page, png in zip(document["pages"], page_pngs, strict=False):
        parts.append(_page_overlay(page, png, document, assets))
    for kind, title in (("image", "Images"), ("vector", "Vector artwork")):
        cards = [_card(i, a, thumbs.get(i)) for i, a in enumerate(assets) if a["kind"] == kind]
        if cards:
            parts.append(f"<h2>{title}</h2><div class=grid>{''.join(cards)}</div>")
    rows = [
        f"<tr><td>{e(a['details'].get('name') or a['details'].get('description') or '')}</td>"
        f"<td>{e(a['kind'])}</td><td><a href='../{e(a['file'])}'>{e(a['file'])}</a></td>"
        f"<td>{e(', '.join(map(str, a['details'].get('pages', a['details'].get('roles', [])))))}</td></tr>"
        for a in assets
        if a["kind"] in ("font", "icc")
    ]
    if rows:
        parts.append(f"<h2>Fonts and colour profiles</h2><table>{''.join(rows)}</table>")
    for page in document["pages"]:
        lines = [x for x in document["lines"] if x["page"] == page["number"]]
        if lines:
            body = "".join(f"<div>{e(x['text'] or '(unreadable)')}</div>" for x in lines)
            parts.append(
                f"<details><summary>Text of page {page['number']} ({len(lines)} lines)</summary>{body}</details>"
            )
    parts.append(f"<script>{_SCRIPT}</script>")
    return "\n".join(parts)


def _summary(summary: dict) -> str:
    def show(value: typing.Any) -> str:
        return json.dumps(value) if isinstance(value, dict | list) else str(value)

    rows = "".join(
        f"<tr><th>{html.escape(key.replace('_', ' '))}</th><td>{html.escape(show(value))}</td></tr>"
        for key, value in summary.items()
        if value is not None
    )
    return f"<details open><summary>Layout summary</summary><table>{rows}</table></details>"


def _page_overlay(page: dict, png: str, document: dict, assets: list[dict]) -> str:
    e = html.escape
    media = page["media_pt"] or [0, 0, page["width_pt"] or 1, page["height_pt"] or 1]
    width, height = media[2] - media[0], media[3] - media[1]
    number = page["number"]

    def box(x: float, y: float, w: float, h: float, cls: str, label: str, href: str | None = None) -> str:
        style = (
            f"left:{(x - media[0]) / width * 100:.3f}%;bottom:{(y - media[1]) / height * 100:.3f}%;"
            f"width:{w / width * 100:.3f}%;height:{h / height * 100:.3f}%"
        )
        tag = f"a href='{href}'" if href else "div"
        return f"<{tag} class='ov {cls}' style='{style}' title='{e(label)}'></{tag.split()[0]}>"

    overlays: list[str] = []
    if not page["rotate"]:  # a rotated page renders turned; the boxes would not line up
        for i, a in enumerate(assets):
            cls = {"image": "img", "vector": "vec"}.get(a["kind"], "")
            overlays += [
                box(p["x"], p["y"], p["width"], p["height"], cls, a["file"], f"#a{i}")
                for p in a["placements"]
                if cls and p["page"] == number
            ]
        overlays += [
            box(x["x"], x["y"], x["width"], x["height"], "txt", x["text"] or "(unreadable)")
            for x in document["lines"]
            if x["page"] == number
        ]
        overlays += [
            box(
                t["x"],
                t["y"],
                t["width"],
                t["height"],
                "tbl",
                f"{t['id']}: {t['rows']} x {t['cols']} {t['mode']} table",
            )
            for t in document["layout"]["pages"][number - 1]["tables"]
        ]
        overlays += [
            box(b["x"], b["y"], b["width"], b["height"], "blk", f"{b['role']}{' (running)' if b['running'] else ''}")
            for b in document["layout"]["pages"][number - 1]["blocks"]
        ]
        overlays += [
            box(x["x"], x["y"], x["width"], x["height"], "box", x["kind"])
            for x in document["boxes"]
            if x["page"] == number
        ]
    return (
        f"<h2 id='page-{number}'>Page {number}</h2><div class=page style='aspect-ratio:{width}/{height}' "
        f"data-x0='{media[0]}' data-y1='{media[3]}' data-w='{width}' data-h='{height}' data-page='{number}'>"
        f"<img src='{e(png)}' alt='page {number}' loading=lazy>{''.join(overlays)}</div>"
    )


def _card(index: int, asset: dict, thumb: str | None) -> str:
    e = html.escape
    where = "; ".join(
        f"p{p['page']} ({p['x']:.0f}, {p['y']:.0f}) {p['width']:.0f}x{p['height']:.0f}pt"
        for p in asset["placements"][:6]
    )
    more = f" +{len(asset['placements']) - 6} more" if len(asset["placements"]) > 6 else ""
    picture = f"<img src='{e(thumb)}' alt=''>" if thumb else "(no preview)"
    detail = (
        " x ".join(map(str, asset["details"]["pixels"])) + " px"
        if asset["kind"] == "image"
        else f"{asset['details']['shapes']} shape(s)"
    )
    return (
        f"<div class=card id='a{index}'><div class=thumb>{picture}</div>"
        f"<a href='../{e(asset['file'])}'>{e(asset['file'])}</a><div class=meta>{e(detail)}</div>"
        f"<div class=meta>{e(where)}{more}</div></div>"
    )
