"""Generates a small test manga CBZ with Japanese text bubbles."""

import zipfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUT = Path(r"D:\Codex\e2e-in\chapter-test.cbz")
TMP = OUT.parent / "_pages"
FONT_PATH = r"C:\Windows\Fonts\msgothic.ttc"

PAGES = [
    (
        "001.png",
        [
            (60, 60, 380, 180, "今日は暑いな…"),
            (160, 480, 520, 620, "アイスを買おう！"),
        ],
    ),
    (
        "002.png",
        [
            (80, 120, 420, 260, "バニラにする。"),
            (200, 520, 540, 660, "甘いものは正義！"),
        ],
    ),
    (
        "010.png",
        [
            (100, 80, 460, 220, "帰り道に寄ろう。"),
            (140, 500, 500, 640, "まずい、財布を忘れた。"),
        ],
    ),
]


def draw_page(draw: ImageDraw.ImageDraw, bubbles) -> None:
    for x0, y0, x1, y1, text in bubbles:
        draw.ellipse((x0, y0, x1, y1), fill="white", outline="black", width=4)
        font = ImageFont.truetype(FONT_PATH, 34)
        box = draw.textbbox((0, 0), text, font=font)
        width = box[2] - box[0]
        center_x = (x0 + x1 - width) / 2
        center_y = (y0 + y1) / 2 - (box[3] - box[1]) / 2
        draw.text((center_x, center_y), text, fill="black", font=font)


def main() -> None:
    TMP.mkdir(parents=True, exist_ok=True)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    for name, bubbles in PAGES:
        page = Image.new("RGB", (600, 800), "#f2ede4")
        draw = ImageDraw.Draw(page)
        draw.rectangle((20, 20, 580, 780), outline="#444", width=2)
        draw_page(draw, bubbles)
        page.save(TMP / name)
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_STORED) as archive:
        for name, _ in PAGES:
            archive.write(TMP / name, arcname=name)
    print(f"wrote {OUT} with {len(PAGES)} pages")


if __name__ == "__main__":
    main()
