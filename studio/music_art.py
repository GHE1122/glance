"""The music video's pictures, both original: the song's cover (a city at
dusk, its windows lit one by one) and the desktop it plays over (the same
evening out of focus: a deep sky, the city's lights as soft discs).

    python studio/music_art.py OUT-FOLDER WIDTH HEIGHT

Writes cover.png (1000 px square) and wallpaper.png (WIDTH x HEIGHT).
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

SKY_TOP = np.array([0.06, 0.05, 0.24])
SKY_MIDDLE = np.array([0.44, 0.14, 0.66])
SKY_LOW = np.array([1.00, 0.38, 0.58])
WARM = [(255, 214, 140), (255, 188, 112), (255, 236, 186), (250, 160, 96)]


def sky(width, height, horizon):
    """Night above, the last of the sun along the horizon (at `horizon`, a share of the height)."""
    y = np.linspace(0, 1, height)[:, None, None] / horizon
    upper = np.clip(y / 0.6, 0, 1)
    lower = np.clip((y - 0.6) / 0.4, 0, 1)
    colour = SKY_TOP + (SKY_MIDDLE - SKY_TOP) * upper ** 1.4
    colour = colour + (SKY_LOW - colour) * lower ** 1.8
    # Below the horizon, into the dark of the ground.
    below = np.clip((y - 1) / 0.25, 0, 1)
    colour = colour + (SKY_LOW * 0.22 - colour) * below
    return np.broadcast_to(colour, (height, width, 3)).copy()


def cover(path, side=1000, seed=7):
    rng = np.random.default_rng(seed)
    image = Image.fromarray((sky(side, side, 0.74) * 255).astype(np.uint8))
    stars = ImageDraw.Draw(image)
    for _ in range(90):
        x, y = rng.uniform(0, side), rng.uniform(0, side * 0.45)
        r = rng.uniform(0.6, 1.8)
        stars.ellipse((x - r, y - r, x + r, y + r), fill=(255, 255, 255, int(rng.uniform(90, 220))))
    # A sun gone down: a glow on the horizon.
    glow = Image.new("RGB", (side, side), (0, 0, 0))
    ImageDraw.Draw(glow).ellipse((side * 0.28, side * 0.62, side * 0.72, side * 0.86), fill=(255, 150, 90))
    image = Image.blend(image, Image.composite(glow, image, glow.convert("L").filter(ImageFilter.GaussianBlur(90))), 0.7)
    # The city: towers, far ones paler, each with rows of windows, some lit.
    city = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    draw = ImageDraw.Draw(city)
    for layer, (shade, base, tall, lit) in enumerate([((60, 30, 92), 0.80, 0.22, 0.06), ((30, 16, 56), 0.88, 0.32, 0.12), ((12, 8, 28), 0.96, 0.42, 0.18)]):
        x = -rng.uniform(0, 40)
        while x < side:
            width = rng.uniform(60, 150) * (1 + layer * 0.3)
            top = side * (base - rng.uniform(0.25, 1.0) * tall)
            draw.rectangle((x, top, x + width, side), fill=shade + (255,))
            cell = 14 + layer * 4
            for wy in np.arange(top + cell, side - cell, cell * 1.6):
                for wx in np.arange(x + cell * 0.6, x + width - cell, cell * 1.3):
                    if rng.random() < lit:
                        colour = WARM[rng.integers(len(WARM))]
                        draw.rectangle((wx, wy, wx + cell * 0.7, wy + cell * 0.9), fill=colour + (int(150 + 100 * layer / 2),))
            x += width + rng.uniform(-10, 18)
    bloom = city.filter(ImageFilter.GaussianBlur(10))
    image = image.convert("RGBA")
    image.alpha_composite(bloom)
    image.alpha_composite(city)
    image.convert("RGB").save(path)


def wallpaper(path, width, height, seed=11):
    rng = np.random.default_rng(seed)
    image = Image.fromarray((sky(width, height, 0.8) * 255 * 0.8).astype(np.uint8)).convert("RGBA")
    # The city's lights as soft discs, more and warmer toward the bottom.
    for blur, count, (low, high) in ((28, 46, (60, 150)), (12, 70, (24, 70))):
        lights = Image.new("RGBA", (width, height), (0, 0, 0, 0))
        draw = ImageDraw.Draw(lights)
        for _ in range(count):
            y = height * (0.45 + 0.55 * rng.random() ** 0.6)
            x = rng.uniform(-0.05, 1.05) * width
            r = rng.uniform(low, high)
            colour = WARM[rng.integers(len(WARM))] if rng.random() < 0.75 else (150, 170, 255)
            draw.ellipse((x - r, y - r, x + r, y + r), fill=colour + (int(rng.uniform(40, 110)),))
        image.alpha_composite(lights.filter(ImageFilter.GaussianBlur(blur)))
    image.convert("RGB").save(path)


def main():
    folder = Path(sys.argv[1])
    width, height = int(sys.argv[2]), int(sys.argv[3])
    folder.mkdir(parents=True, exist_ok=True)
    cover(folder / "cover.png")
    wallpaper(folder / "wallpaper.png", width, height)


if __name__ == "__main__":
    main()
