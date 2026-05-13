# Scorecard Analysis

## Local Development

Run Scorecard locally without signing:

```bash
./scripts/run-scorecard.sh
```

Or with custom repo/output:

```bash
./scripts/run-scorecard.sh victron-venus/inverter-desktop custom-results.sarif
```

## GitHub Actions

Scorecard runs automatically on:
- Push to `main` branch
- Pull requests to `main` branch  
- Weekly schedule (Mondays 6:00 UTC)

Results are signed using GitHub Actions OIDC and uploaded to GitHub Security tab.

## Troubleshooting

### "expired_token" error locally

This happens when running Scorecard with signing enabled locally. The device flow requires browser interaction within 5 minutes.

**Solution:** Use the provided script which disables signing for local runs. GitHub Actions handles signing automatically.

### Missing scorecard CLI

Install Scorecard CLI:

```bash
go install github.com/ossf/scorecard/v4/cmd/scorecard@latest
```

## Configuration

- `.scorecard.yaml` - Local configuration (signing disabled)
- `.github/workflows/scorecard.yml` - GitHub Actions workflow (signing enabled via OIDC)
