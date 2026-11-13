# CLAUDE.md

`paper2html`: a Rust CLI and library that converts arXiv PDF papers to HTML, in 4 modes:

- `hard`: a copy of every page, with a selectable text layer.
- `hard-95`: like `hard`, but all text is real HTML text placed as in the PDF.
- `medium`: page and column layout kept, text reflowed.
- `friendly`: a single-column article, with a scroll view, a book view and a phone layout.

Read before changing code:

- [ARCHITECTURE.md](ARCHITECTURE.md): pipeline, modules, coding conventions.
- [CONTRIBUTING.md](CONTRIBUTING.md): commit messages.
- [README.md](README.md), the "Testing" section.

## Commands

```bash
cargo build --release
./target/release/paper2html convert <file.pdf | arXiv id> -m friendly -o out/x.friendly.html   # or --all -o out/dir
./target/release/paper2html inspect <file.pdf>        # print the detected structure, for debugging
scripts/fetch-test-pdfs.sh                            # download the 7 test papers (once)
cargo test --release                                  # unit tests + 8 real papers + snapshots
P2H_UPDATE_SNAPSHOTS=1 cargo test --release --test real_papers   # when output changes ON PURPOSE
P2H_PRINT_METRICS=1 cargo test --release --test real_papers -- --nocapture  # print each paper's metrics
cargo fmt --check && cargo clippy --all-targets -- -D warnings      # the CI lint gate
node tests/browser/check-friendly.js out/*.friendly.html             # browser checks
node tests/browser/check-hard95.js out/*.hard-95.html
node tests/browser/check-stable.js out/*.medium.html out/*.friendly.html
```

Chromium and Playwright are preinstalled in the cloud environment. Do not run
`playwright install`.

## Automatic hooks (`.claude/settings.json`)

- **After each Edit/Write** (`.claude/hooks/after-edit.sh`):
  - `.rs`: runs `rustfmt`.
  - `.js`: runs `node --check`.
  - `.json`: checks that the JSON is valid.
  - `.sh`: runs `bash -n`.

  Errors (syntax errors, rustfmt failures) are reported back at once so they can be fixed.
- **At the end of a turn** (`.claude/hooks/on-stop.sh`): runs only if the turn changed a `.rs`
  file.
  1. Runs `cargo fmt`, then `cargo clippy --fix`, then `cargo clippy --all-targets -- -D warnings`.
  2. Warnings that clippy cannot fix itself are sent back to Claude to finish (once per turn).

The hooks do not run the tests, which are slow. Run `cargo test --release` yourself before
committing.

## Workflow for changes to the analysis (`src/analysis`)

- A new threshold must be a documented field in that stage's config struct (`src/config.rs`). The
  field name ends with its unit: `_em`, `_pt` or `_ratio`. No magic numbers in code.
- A rule that fixes one paper easily breaks another. After each change:
  1. Run the tests again and see which snapshots changed.
  2. Compare the output text before and after on as many papers as you can (the 8 test papers,
     plus other varied papers if you have them).
- Snapshots (`tests/data/snapshots/*.txt`) are hashes of each mode's HTML.
  - Changing the renderer's CSS, JS or assets also changes the snapshots, because they are
    embedded with `include_str!`.
  - Update the snapshots only when the output change is intended, and commit them with the code.
- The limits in `tests/real_papers.rs` (broken paragraphs, words still hyphenated, leftover
  characters, ...) matter: when a number gets better, tighten the limit; when it gets worse, it is
  a regression.
- A test for a specific bug goes in `tests/real_papers.rs` and is named after the behaviour, for
  example `ligo_radical_is_a_square_root`.

## Renderer JS and CSS

- `src/render/friendly/{friendly.js,book.js,friendly.css}` and `src/render/hard95/` are written
  compactly on purpose. Do not run prettier or reformat whole files: the diff would be huge and
  every snapshot would change.
- The book view (`book.js`) measures layout with CSS columns. Any change to page breaking must be
  followed by `check-friendly.js`, which checks for near-empty pages, captions apart from their
  figures, and orphaned headings.

## Constraints

- **PDFs:** only the LIGO paper (`tests/data/1602.03837v1.pdf`, CC BY 4.0) is committed. Other
  arXiv papers may not be redistributed: download them with `scripts/fetch-test-pdfs.sh` and keep
  them out of git (`.gitignore` already has `*.pdf`).
- **Clippy:** CI uses a newer clippy than the local one, so it can report extra lints (for example
  `needless_bool`). Check the CI log if the "fmt + clippy" job fails.
- **Commits:** Conventional Commits (`fix(tables): ...`, `feat: ...`), in English, imperative
  mood.

## Pull requests

- Title and description in English.
- **Assignee:** always `iamcaominhtien`.
- **Labels:** add them yourself from what the PR contains, one or more. Use the repository's
  existing labels:

  | PR content | Label |
  | --- | --- |
  | New feature or an improvement to the output (`feat`, `perf`) | `enhancement` |
  | Bug fix (`fix`) | `bug` |
  | Documentation (`docs`, README, ARCHITECTURE, CLAUDE.md) | `documentation` |

  A PR that mixes kinds gets every label that applies. If a PR fits none of these (for example
  CI only, or refactoring only), create a short, lowercase label that names it (`ci`,
  `refactor`, `tests`) rather than leaving the PR unlabelled.
