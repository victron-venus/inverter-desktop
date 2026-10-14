# Project governance

Inverter Desktop is maintained by **4alvit (alvit)**. The maintainer decides scope,
reviews contributions, maintains repository access and CI, and accepts releases.
Contributors propose focused changes through public issues and pull requests;
maintainer changes follow the same tests and review process. Automated review and
release accounts assist this process but are not independent human maintainers.

## Decisions and responsibilities

Discuss architectural or incompatible changes in a GitHub issue before a large
implementation. A pull request records the problem, compatibility impact,
validation and reviewer discussion. The maintainer resolves disagreements with a
reason recorded in that discussion. Security-sensitive matters instead follow
[SECURITY.md](SECURITY.md), and conduct complaints follow
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

Contributors are responsible for compatible licensing, tests, documentation and
sanitized reproductions. Reviewers check behavior, failure paths and evidence.
The release owner verifies the exact candidate, records target acceptance and
follows [RELEASING.md](RELEASING.md). CI success alone is not device acceptance.
No mandatory DCO sign-off or CLA is currently imposed; the contribution license
requirements remain in [CONTRIBUTING.md](CONTRIBUTING.md).

## Continuity limitation

There is currently **no designated backup maintainer or established mechanism**
that can restore repository, CI and release access within one week if the primary
maintainer becomes unavailable. Public source copies and automation do not solve
that access problem. OpenSSF `access_continuity` and the two-person `bus_factor`
criterion therefore remain unmet. A future successor needs actual access,
responsibilities and a verified recovery exercise before this statement changes.

## Roadmap and achievements

[TODO.md](TODO.md) records the evolving roadmap and implementation status.
[CHANGELOG.md](CHANGELOG.md) and the public
[releases](https://github.com/victron-venus/inverter-desktop/releases) record
achievements and upgrade impact. Public issues can propose priorities; no support
duration or delivery date is promised by a roadmap entry.
