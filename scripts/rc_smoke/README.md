# Published Linux RC acceptance

This harness checks the actual published Debian package in an isolated Linux user session. It does not rebuild the application. It verifies the package digest, extracts it without installation, checks the executable digest before launch, and requires visible Setup wizard content and rendered pixels. A fresh profile, private D-Bus and Secret Service, network namespace, and bounded process cleanup isolate the run.

The check covers Linux first-run software behavior. It does not establish acceptance of macOS, Windows, mobile targets, existing user profiles, upgrades, devices, or MQTT services.

## Reusing the harness

The callable job is intentionally inactive outside the dedicated, source-bound draft PR. It does not run on ordinary PRs or release events after that PR is merged.

For another release candidate:

1. Complete the normal RC producer and verify its full published asset and provenance contracts. Preserve that acceptance receipt.
2. Create the `test/published-rc-linux-smoke` branch from the expected release source. Keep the resulting PR in draft without auto-merge while the source remains the release cutoff.
3. Update `binding.json` with the exact repository, source commit, RC tag, successful run and attempt, release ID, Debian asset ID/name/size/SHA-256, and acceptance receipt digest. Set activation to `ACCEPTED_RC` only after these values have been checked against the actual published bytes. Verify the bound lifetime-helper digest against that source too.
4. Update the fixed base commit in both the `ci.yml` caller condition and `rc-artifact-smoke.yml` callee condition. Keep the same-repository, exact-branch, and `pull_request` restrictions. Match the Linux runner and runtime packages to the actual package's build ABI.
5. Run the inert smoke contracts, the repository's actual `scripts/workflow_contracts.py`, and actionlint. Review the source/binding changes before publication. The required Quality gate must include the callable smoke result.
6. Accept only the actual current-head hosted run: exact package/source binding, rendered readiness, and confirmed process, namespace, package, and scratch cleanup. Preserve its report, screenshot, and digests. A skipped job, inert test pass, or process/window existence alone is not application acceptance.
7. Keep the main source unchanged while qualifying that RC for stable. If stable is authorized, preserve it through promotion and byte verification, then merge the reviewed harness update through normal protected CI. If stable is deferred, follow the explicit deferred-stable procedure below.

Do not relax the sandbox, disable WebKit's own sandbox, use a normal user profile, or raise deadlines merely to make a failed check pass. Diagnose the observed failure and review any required change.

## When stable is deferred

The release owner may record the selected RC's completed software checks and unresolved acceptance requirements, then choose normal reviewed maintenance delivery without promoting that RC. Merge the maintained harness and compatible maintenance changes through full source CI, and let the existing release policy produce a new beta and RC from the resulting source. Do not change versions solely to bypass an incomplete acceptance gate.

Retained evidence remains bound to the old RC bytes and source. In particular, a successful smoke of source `5b264792f4e5e1d089c53bbfa5c2234f27a0f99d` does not exercise dependencies changed in a later source, even if that later PR runs the same old-package smoke. Before any later stable promotion, rebind and execute the actual new RC package using the procedure above, and satisfy all other applicable release criteria. Deferring stable or merging this harness does not waive physical-device or MQTT acceptance.

## Runtime and evidence bounds

Runtime children inherit a zero core-dump limit and a finite 256 MiB per-file size limit. The latter also applies to anonymous file-backed shared memory such as `memfd`, not only disk output. It matches the existing whole-scratch ceiling rather than imposing the former 1 MiB log limit on WebKit shared-memory files. It is not a total resident-memory or total shared-memory quota. The inert probe records the actual inherited soft/hard limits and its result; that probe does not decide application acceptance.

Each service's stdout/stderr is drained continuously, retaining at most a 1 MiB prefix and reporting discarded bytes. A shared bounded shutdown deadline closes log readers even if a late descendant keeps a pipe open. The independent outer supervisor still monitors 256 MiB of scratch and 8 MiB of runtime evidence, with the original 180-second outer and 120-second inner deadlines. Those aggregate scans are monitored limits, not per-write filesystem quotas. Do not enlarge a limit again merely to make a failed run pass; preserve the failure and investigate its actual boundary.
