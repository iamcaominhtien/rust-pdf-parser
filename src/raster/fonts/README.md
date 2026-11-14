# TeX fonts of the formula reader

`paper2html formula` compares the ink of a formula picture with the glyphs of the fonts LaTeX
sets formulas in, at the sizes it uses (10 pt text, 7 pt scripts, 5 pt scripts of scripts, each
with strokes of its own). These are the Type 1 files, unchanged, read with skrifa.

| Files | Fonts | Source (CTAN) | License |
| --- | --- | --- | --- |
| `cmr`, `cmmi`, `cmsy` (10, 7, 5), `cmex` (10, 7), `cmbx`, `cmmib` (10, 7) | Computer Modern: roman, math italic, symbols, extension, bold, bold math italic | `fonts/amsfonts/pfb` | SIL Open Font License 1.1 (`OFL.txt`) |
| `msbm` (10, 7, 5), `msam10`, `eufm` (10, 7, 5) | AMS symbol fonts (blackboard bold, more relations) and Euler Fraktur | `fonts/amsfonts/pfb` | SIL Open Font License 1.1 (`OFL.txt`) |
| `dsrom10` | `dsfont` double-stroke letters | `fonts/doublestroke` | LaTeX Project Public License 1.3c |
| `rsfs` (10, 7) | Ralph Smith's Formal Script (`\mathscr`) | `fonts/rsfs/type1` | LaTeX Project Public License 1.3c |
