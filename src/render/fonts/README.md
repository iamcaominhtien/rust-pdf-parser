# Math font of the medium and friendly modes

Embedded (base64 `@font-face`) in medium and friendly pages that have formulas written as MathML,
unless `--fonts system` is given. Readers without a math font of their own (Linux, Android) would
otherwise see the formulas drawn with a text font.

| File | Font | Subset |
| --- | --- | --- |
| `latin-modern-math.woff2` | Latin Modern Math 1.959 (B. Jackowski, P. Strzelczyk and P. Pianowski, for the TeX users groups), the math font of LaTeX, from CTAN `fonts/lm-math` | Latin, Latin-1, Greek, combining and spacing accents, punctuation, letterlike symbols, arrows, math operators, technical symbols (brackets and their pieces), misc. math symbols A, supplemental math operators, math alphanumerics (serif, script, fraktur, double-struck, bold digits); OpenType features `ssty`, `dtls`, `flac`, `kern`, `mark`, `mkmk`, `ccmp`; the `MATH` table (stretchy variants and assemblies) kept; hinting removed |

Licensed under the GUST Font License (see `GUST-FONT-LICENSE.txt`), an instance of the LaTeX
Project Public License. The subset keeps the family name "Latin Modern Math".

The subset was made with fontTools:

```bash
pyftsubset latinmodern-math.otf --flavor=woff2 --no-hinting --desubroutinize \
  --layout-features='ssty,dtls,flac,kern,mark,mkmk,ccmp' \
  --unicodes='U+0020-007E,U+00A0-00FF,U+0131,U+0237,U+02C6-02DD,U+0300-036F,U+0391-03A9,U+03B1-03C9,U+03D1,U+03D5,U+03D6,U+03F0,U+03F1,U+03F5,U+2000-206F,U+20D0-20F0,U+2100-214F,U+2190-21FF,U+2200-22FF,U+2300-23FF,U+27C0-27EF,U+2A00-2AFF,U+1D400-1D59F,U+1D6A8-1D755,U+1D7CE-1D7E1' \
  --output-file=latin-modern-math.woff2
```
