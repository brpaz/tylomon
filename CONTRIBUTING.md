# Contributing to tylomon

Thanks for your interest in tylomon. Bug reports, feature ideas, documentation fixes and code are all welcome.

tylomon is largely written with AI coding assistants (see the disclosure in the [README](README.md)). Contributions made with AI help are fine; you are responsible for understanding, testing and standing behind what you submit.

By contributing you agree that your work is released under the [MIT license](LICENSE). Please be respectful and constructive in issues and reviews.

## Reporting issues

Search the [existing issues](https://github.com/brpaz/tylomon/issues) first, including closed ones. If you find a match, add what you know to it instead of opening a new one.

### Bug reports

Open an issue with a title that says what is wrong, and include:

- **Version and install method**: output of `tylomon --version`, and whether you built from source, used `task install` or run the Docker image
- **Environment**: Linux distribution and kernel, Docker version, and how tylomon gets its privileges (`sudo`, `setcap`, container capabilities)
- **What you ran**: the exact command line, plus relevant environment variables such as `COMPOSE_FILE`
- **What you expected and what happened**: error messages verbatim, and a screenshot or terminal capture for display problems
- **How to reproduce it**: ideally with the sample stack in `examples/`, or a minimal compose file
- **Logs**: the log file (`tylomon.log` in the working directory by default). Reproduce with `RUST_LOG=tylomon=debug` for more detail
- **A recording, if the problem is in decoding**: `tylomon --project <name> --write bug.pcap` captures the traffic so the problem can be reproduced with `tylomon --read bug.pcap` and no Docker. **Recordings contain the raw traffic, including bodies, headers and SQL parameters. Remove or replace anything sensitive, or share it privately.**

### Feature requests

Describe the problem you are trying to solve before the solution you have in mind, and say how you work around it today. For a new protocol, include a short sample of the traffic (a pcap or a compose stack that produces it) and say whether it is cleartext, since encrypted traffic cannot be decoded.

### Security issues

Please do not report vulnerabilities in public issues. Use GitHub's private vulnerability reporting (the **Security** tab of the repository, **Report a vulnerability**). If that is not available, open an issue that asks for a private contact without describing the problem.

tylomon needs `CAP_NET_RAW` and `CAP_NET_ADMIN` and parses untrusted network data, so reports about the decoders (malformed packets causing panics, excessive memory use and so on) are welcome.

## Development setup

### Prerequisites

The project uses [devenv.sh](https://devenv.sh/) to manage the development environment. With devenv installed:

```bash
devenv shell
```

This provides the Rust toolchain, libpcap, [Task](https://taskfile.dev/), [Lefthook](https://github.com/evilmartians/lefthook), `uv`, cargo-watch and cargo-nextest.

Without devenv you need the stable Rust toolchain pinned in `rust-toolchain.toml`, the libpcap development headers (`libpcap-dev` or `libpcap-devel`) and `pkg-config`. Task, `uv` and Docker are needed for the tasks below.

### Common tasks

```bash
task fmt            # Format code
task lint           # Run fmt check + clippy
task test           # Run tests
task build          # Build debug binary
task build:release  # Build release binary
task install        # Build release, install it and grant capabilities (INSTALL_DIR=, SETCAP=)
task run -- --project myapp  # Build, grant capabilities, run
task watch          # Watch and rebuild on changes
task e2e            # Capture the examples/ stack and assert on the exchanges
task ci             # Run the full CI pipeline locally
task docker:build   # Build Docker image locally
task docs:serve     # Preview the documentation (Zensical)
```

Live capture needs `CAP_NET_RAW` and `CAP_NET_ADMIN`. `task run` and `task setcap` grant them to the debug binary with `sudo setcap`; they are lost on every rebuild.

### Tests

- Unit tests live next to the code in `#[cfg(test)] mod tests`.
- `tests/replay_tests.rs` builds pcap files with real packets and replays them through the capture pipeline, so it needs neither privileges nor Docker. Use it for anything that touches the decoders.
- `task e2e` (`scripts/e2e.sh`) captures the stack in `examples/` and checks the output. It needs Docker, `jq` and `sudo`.

Run `cargo test --all-features` before pushing. New parsing and decoding code needs tests, including malformed and truncated input.

### Git hooks

[Lefthook](https://github.com/evilmartians/lefthook) is configured with:

- **pre-commit**: format check
- **pre-push**: format check, clippy, tests

Hooks are installed automatically when entering the devenv shell.

### Code style and architecture

- Formatting is plain `cargo fmt`; clippy runs with `-D warnings`.
- The [code style guide](docs/development/contributing.md) covers imports, naming, error handling and TUI conventions.
- The README has an overview of the source tree, and the [architecture page](docs/reference/architecture.md) explains the data pipeline.
- Documentation lives in `docs/` and is built with [Zensical](https://zensical.org/) (`task docs:serve`, `task docs:build`). Update it, the README and the `?` help overlay in the same change as any user-visible behaviour, including new key bindings.

## Development process

### From idea to merge

1. **Discuss.** For anything bigger than a small fix, open or comment on an issue first so the approach can be agreed before you spend time on it.
2. **Branch.** Work on a branch from `main`, named for the change: `feat/...`, `fix/...`, `docs/...`, `chore/...`.
3. **Commit.** Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `chore(deps):`, `ci:`, `build:` …) and keep each commit focused. Mark breaking changes with `!` or a `BREAKING CHANGE:` footer.
4. **Check locally.** Run `task ci`, and `task e2e` if you touched capture, decoding, export or the example stack.
5. **Open a pull request** against `main`. Explain what changed and why, link the issue, and say how you tested it. Keep PRs to one feature or fix.
6. **Review and CI.** All checks must pass and the changes need to be understood and agreed by a maintainer. Address review comments with new commits; they are squashed on merge.
7. **Merge.** Pull requests are squash-merged into a single logical commit on `main`, using a Conventional Commit title.

### Definition of done

- Behaviour change is covered by tests, and `cargo fmt`, clippy and the tests pass
- Docs, README, `?` help overlay and keybindings reference are updated where relevant
- No secrets, captured traffic or environment files with real values are committed
- New dependencies are justified, pinned to an exact version and checked for known vulnerabilities

### Continuous integration

GitHub Actions runs on every push to `main` and on pull requests:

| Job | What it does |
|-----|--------------|
| **Check & Test** | `cargo fmt --check`, clippy with `-D warnings`, `cargo test` |
| **End-to-end** | Builds the binary and runs `scripts/e2e.sh` against the example stack |
| **Build Binary** | Release build; uploaded as an artifact, and attached to the release when one is published |
| **Build & Push Docker Image** | Multi-arch image (amd64, arm64) pushed to `ghcr.io`; runs for published releases only |
| **Docs** | Builds the site with `--strict` and deploys it to GitHub Pages when `docs/`, `zensical.toml` or `requirements-docs.txt` change on `main` |
| **Draft release** | On pushes to `main`, [draftsman](https://github.com/brpaz/draftsman) updates the draft release notes from the Conventional Commits since the last release |

### Dependencies

[Renovate](https://docs.renovatebot.com/) opens pull requests for dependency, GitHub Action and Docker base image updates. Versions are pinned exactly (and Actions by commit SHA), updates wait three days after release, and vulnerability alerts are raised straight away. Minor and patch updates of actions are grouped; Rust crate and Docker updates are grouped and need a maintainer to merge. A lock-file refresh runs monthly.

Do not bump versions by hand unless a change needs it. If you add a dependency, pin it with `=` in `Cargo.toml`.

### Releases

Releases follow [semantic versioning](https://semver.org/).

1. [draftsman](https://github.com/brpaz/draftsman) maintains a draft release. On every push to `main` it reads the Conventional Commits since the last release, groups them into sections by type (`feat` is Features, `fix` is Bug Fixes, `chore`, `build` and `ci` are Maintenance, `docs` is Documentation, anything else is Other), and picks the next version itself: a breaking change (`!` or a `BREAKING CHANGE:` footer) is a major bump, `feat` a minor one, anything else a patch. The sections are configured in [`.draftsman.yml`](.draftsman.yml).
2. A maintainer checks the draft and publishes it (from the GitHub release page, or with `draftsman publish`), which creates the `vX.Y.Z` tag.
3. Publishing triggers CI to set the version in `Cargo.toml` from the tag, build the release binary and attach it to the release, and build and push the multi-arch Docker image to `ghcr.io/brpaz/tylomon`.

The release notes are built from commit messages, not labels, so the squash-merge title of a pull request (which becomes the commit on `main`) must be a clear Conventional Commit. To keep a commit out of the release notes, add `[skip changelog]` anywhere in its message or a `Skip-Changelog: true` footer.
