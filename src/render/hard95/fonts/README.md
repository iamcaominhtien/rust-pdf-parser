# Text fonts of the hard-95 mode

Embedded (base64 `@font-face`) in hard-95 pages of papers set in Computer Modern, or whose formulas
use the TeX math fonts, unless `--fonts system` is given. With the fonts of LaTeX the text in the
page has the letter shapes and almost exactly the widths of the PDF, so that the fraction bars and
radicals drawn in the page picture stay over their text. The math symbols come from Latin Modern
Math (`../../fonts/latin-modern-math.woff2`).

| File | Font |
| --- | --- |
| `lmroman10-regular.woff2` | Latin Modern Roman 10 Regular |
| `lmroman10-italic.woff2` | Latin Modern Roman 10 Italic |
| `lmroman10-bold.woff2` | Latin Modern Roman 10 Bold |
| `lmroman10-bolditalic.woff2` | Latin Modern Roman 10 Bold Italic |
| `lmmono10-regular.woff2` | Latin Modern Mono 10 Regular |

Latin Modern 2.004 (B. Jackowski and J. M. Nowacki, for the TeX users groups), from CTAN
`fonts/lm/fonts/opentype/public/lm`. Licensed under the GUST Font License (see
`GUST-FONT-LICENSE.txt`), an instance of the LaTeX Project Public License. The family is renamed in
the pages ("P2H LM Roman", "P2H LM Mono"), so that a copy installed on the reader's machine, with
other vertical metrics, is never used in their place.

Two changes were made to each font:

1. Subset with fontTools to Basic Latin, Latin-1, Latin Extended-A, spacing and combining accents,
   general punctuation, the euro sign, the trade mark sign, simple arrows and the minus sign:

   ```bash
   pyftsubset lmroman10-regular.otf --flavor=woff2 --no-hinting --desubroutinize \
     --layout-features='kern,mark,mkmk,ccmp' \
     --unicodes='U+0020-007E,U+00A0-017F,U+0192,U+0218-021B,U+02C6-02DD,U+0300-036F,U+2000-206F,U+20AC,U+2122,U+2190-2193,U+2212' \
     --output-file=lmroman10-regular.woff2
   ```

2. Vertical metrics set so that, with a line height of 1, the baseline is 0.84 em below the top of
   the line, where hard-95 places it (as for Times): ascender 840, descender -160, no line gap, in
   `hhea`, `OS/2` typo (with `USE_TYPO_METRICS`) and `OS/2` win.

   ```python
   from fontTools.ttLib import TTFont
   font = TTFont("lmroman10-regular.woff2")
   font["hhea"].ascent, font["hhea"].descent, font["hhea"].lineGap = 840, -160, 0
   os2 = font["OS/2"]
   os2.sTypoAscender, os2.sTypoDescender, os2.sTypoLineGap = 840, -160, 0
   os2.usWinAscent, os2.usWinDescent = 840, 160
   os2.fsSelection |= 1 << 7
   font.save("lmroman10-regular.woff2")
   ```
