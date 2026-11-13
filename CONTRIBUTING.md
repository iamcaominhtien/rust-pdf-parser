# Contributing

Code structure and coding conventions: see [ARCHITECTURE.md](ARCHITECTURE.md). How to run the
tests: see the "Testing" section of [README.md](README.md).

## Commit messages: Conventional Commits

The first line of a commit has the form `<type>: <description>` (in English, imperative mood).
The type decides the next version and the group in `CHANGELOG.md`:

| Type | Use for | Version bump (while 0.x) | Changelog group |
| --- | --- | --- | --- |
| `feat` | A new feature | minor: 0.1.0 → 0.2.0 | Added |
| `fix` | A bug fix | patch: 0.1.0 → 0.1.1 | Fixed |
| `perf` | Faster, less memory | patch | Performance |
| `refactor` | Reorganised code, same behaviour | patch | Changed |
| `docs` | Documentation | patch | Documentation |
| `test` | Tests | patch | Tests |
| `ci`, `build` | CI, build, dependencies | patch | Build and CI |
| `chore` | Other chores | patch | Other |

- Breaking changes (a changed CLI option, a changed JSON output format, a changed public API):
  add `!` after the type (`feat!: ...`) or a `BREAKING CHANGE: ...` line in the body.
- A scope may go in parentheses: `fix(tables): keep header rows at the top of a page`.
- Examples: `feat: add a batch command`, `perf: keep embedded JPEGs instead of re-encoding them`.

Older commits from before this convention use the form "Add ...", "Fix ...". The changelog still
puts them in the right group.

## Releases

Releases are automated with [release-plz](https://release-plz.dev) (`release-plz.toml`,
`.github/workflows/release.yml`):

1. Each time `main` gets new commits, the bot opens or updates a **release PR** named
   `chore: release vX.Y.Z`. This PR bumps the version in `Cargo.toml` according to the commits
   since the last release, and adds a new entry to `CHANGELOG.md`. The changelog text in the PR
   can be edited by hand before merging.
2. To release, **merge the release PR**. The workflow creates the tag `vX.Y.Z` and a GitHub
   Release with the changelog text, then builds binaries for Linux (x86_64, aarch64), macOS
   (Apple Silicon, Intel) and Windows (x86_64), with sha256 checksums, and attaches them to the
   release.

Nothing is published to crates.io: the version comes from git tags (`git_only = true`).

### One-time setup on GitHub

- **Settings → Actions → General → Workflow permissions:** choose "Read and write permissions"
  and enable "Allow GitHub Actions to create and approve pull requests". Without this, the bot
  cannot open the release PR.
- **Optional:** create a `RELEASE_PLZ_TOKEN` secret (a fine-grained personal access token for the
  repository, with Contents and Pull requests: read and write). Without it, the workflow uses
  GitHub Actions' default token; CI then **does not run on the release PR by itself** (a GitHub
  limitation for PRs opened with the default token). The release PR only changes `Cargo.toml` and
  `CHANGELOG.md`, and CI still runs again on `main` after the merge.

## Code quality: SonarQube Cloud

`.github/workflows/sonar.yml` sends each push to `main` and each pull request to SonarQube Cloud,
with the clippy findings (`clippy-report.json`) and the test coverage from `cargo llvm-cov`
(`lcov.info`). Settings are in `sonar-project.properties`. The job is skipped until the
`SONAR_TOKEN` secret exists. One-time setup:

1. Sign in to [sonarcloud.io](https://sonarcloud.io) with GitHub, import the GitHub organization
   and add this repository as a project.
2. In the project, **Administration → Analysis Method**: turn off **Automatic Analysis** (the
   workflow does the analysis; both cannot run at once, and only the workflow imports clippy and
   coverage).
3. **My Account → Security**: generate a token, and add it to the repository as the GitHub secret
   `SONAR_TOKEN` (**Settings → Secrets and variables → Actions**).
4. If SonarQube Cloud gave the organization or project another key than the ones in
   `sonar-project.properties`, change them there.
