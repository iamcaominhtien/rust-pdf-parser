# Math font of the medium and friendly modes

Embedded (base64 `@font-face`) in medium and friendly pages that have formulas written as MathML,
unless `--fonts system` is given. Readers without a math font of their own (Linux, Android) would
otherwise see the formulas drawn with a text font.

| File | Font | Subset |
| --- | --- | --- |
| `stix-two-math.woff2` | STIX Two Math 2.12 (The STIX Fonts Project Authors), from CTAN `fonts/stix2-otf` | Latin, Latin-1, Greek, combining and spacing accents, punctuation, letterlike symbols, arrows, math operators, technical symbols (brackets and their pieces), misc. math symbols A, supplemental math operators, math alphanumerics (serif, script, fraktur, double-struck, bold digits); OpenType features `ssty`, `dtls`, `flac`, `kern`, `mark`, `mkmk`, `ccmp`; the `MATH` table (stretchy variants and assemblies) kept; hinting removed |

Licensed under the SIL Open Font License 1.1 (see `OFL-STIX-Two.txt`). Its only Reserved Font
Name is "TM Math", so the subset keeps the family name "STIX Two Math".

The subset was made with fontTools:

```bash
pyftsubset STIXTwoMath-Regular.otf --flavor=woff2 --no-hinting --desubroutinize \
  --layout-features='ssty,dtls,flac,kern,mark,mkmk,ccmp' \
  --unicodes='U+0020-007E,U+00A0-00FF,U+0131,U+0237,U+02C6-02DD,U+0300-036F,U+0391-03A9,U+03B1-03C9,U+03D1,U+03D5,U+03D6,U+03F0,U+03F1,U+03F5,U+2000-206F,U+20D0-20F0,U+2100-214F,U+2190-21FF,U+2200-22FF,U+2300-23FF,U+27C0-27EF,U+2A00-2AFF,U+1D400-1D59F,U+1D6A8-1D755,U+1D7CE-1D7E1' \
  --output-file=stix-two-math.woff2
```
