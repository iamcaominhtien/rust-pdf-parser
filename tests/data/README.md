# Test data

`papers.txt` lists the arXiv papers used by `tests/real_papers.rs`, with pinned versions and
sha256 checksums.

Only papers whose licence allows redistribution are committed to the repository:

- `1602.03837v1.pdf`: B. P. Abbott et al. (LIGO Scientific Collaboration and Virgo Collaboration),
  "Observation of Gravitational Waves from a Binary Black Hole Merger", arXiv:1602.03837,
  licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). The file is kept
  as is, unmodified.

`snapshots/` holds the sha256 checksums of each paper's output (not the papers' content). The
exception is `snapshots/1602.03837v1.json`, the full JSON output for the LIGO paper, that is, a
converted form of that paper; it stays under CC BY 4.0 with the same attribution as above.

The other papers use arXiv's default licence (only arXiv may distribute them), so they are not in
the repository. Download them once into this folder with:

```bash
scripts/fetch-test-pdfs.sh
```

Tests skip any paper that is missing and print the list of missing papers. With the environment
variable `P2H_REQUIRE_ALL_PAPERS=1` (as in CI), a missing paper is an error.
