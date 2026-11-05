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
7. Preserve the main release source through any stable promotion and byte verification. Only then merge the reviewed harness update through normal protected CI.

Do not relax the sandbox, disable WebKit's own sandbox, use a normal user profile, or raise deadlines merely to make a failed check pass. Diagnose the observed failure and review any required change.
