# Code signing policy

**Status: releases are not code-signed yet.** The SignPath Foundation open-source programme declined the project for now (November 2026). The policy below applies again once a certificate is in place (a later SignPath application or a certificate bought by the maintainer).

## What is signed

Only `Wallplayer.exe`, built from this repository by the GitHub Actions workflow
[`.github/workflows/build.yml`](.github/workflows/build.yml) on GitHub-hosted runners.
Nothing built on a personal machine is signed, and no third-party binary is signed
under this project.

The file is published in [Releases](https://github.com/lehuynhtan17/Wallplayer/releases)
as `Wallplayer-Windows-x64.zip`.

## Team roles

| Role | Members |
|---|---|
| Committers and reviewers | [lehuynhtan17](https://github.com/lehuynhtan17) |
| Approvers | [lehuynhtan17](https://github.com/lehuynhtan17) |

- Committers may change the source code directly.
- Every change from someone outside the team (pull requests) is reviewed by a committer before it is merged.
- Every signing request is approved by hand by an approver.
- All team members use multi-factor authentication for GitHub and the signing service.

## Privacy

See the [privacy policy](PRIVACY.md). In short: this program will not transfer any
information to other networked systems unless specifically requested by the user or
the person installing or operating it. The one exception it makes on its own, the
update check at start, can be turned off.
