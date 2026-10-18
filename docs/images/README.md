# Screenshot artwork

The source is the actual native Linux first-launch screen from v2.5.50-rc.3 (commit `ed003bde0d0f4fafa59a27be29221f85fc8e739e`), captured at 1280 × 800 during the isolated AMD64 package run. The preview has not been connected to a device. The renderer removes only the black side margins (800 × 800 at x=240); it preserves the complete setup interface. This is an RC illustration, not a claim that the latest stable release has this exact screen.

`desktop-setup.png` is the retained flat source. Its SHA-256 is
`e3239002784b87a355b2cb282ca6a4b053e649a55a700e940db5875fe72ad61b`.
The README links the perspective hero to this source for readable detail.

`app-hero.png` applies only a geometric perspective projection, rounded outer
corners and a soft shadow on a neutral background. No interface text, controls,
charts or readings are generated or repainted. This artwork is documentation,
not test or release acceptance evidence.

## Regenerate

Install ImageMagick 7, then run from the repository root:

```sh
bash scripts/render-docs-hero.sh
```

The output is a static 1600 × 1050 PNG. The script checks source dimensions,
uses a temporary directory, strips metadata and replaces the hero only after a
successful render. Same source and ImageMagick version produce identical output;
resampling can differ across versions. Initially rendered with ImageMagick 7.1.2-31.

When replacing a screenshot, use synthetic or explicitly public data, record the
application version, review the complete flat image for private information,
and update this provenance and the README caption before regenerating.
