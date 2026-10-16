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

## Pull requests

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

## More

The rest of the contributor documentation is in [`docs/`](docs/):

- **[Development Setup](docs/development/setup.md)**: environment, Task commands, tests and git hooks
- **[Contributing guide](docs/development/contributing.md)**: CI, dependency updates, how releases are made, and the code style
- **[Architecture](docs/reference/architecture.md)**: source tree and data pipeline
