import * as pulumi from '@pulumi/pulumi'
import * as github from '@pulumi/github'

// Project-level config for secrets that must be set via `pulumi config set --secret`.
const cfg = new pulumi.Config()

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

// Bring the existing GitHub repository under Pulumi management.
// On the first `pulumi up` this import will reconcile state without recreating.
//
// Required before deploying:
//   pulumi config set --secret github:token <your-personal-access-token>
//   (Scopes needed: repo, admin:repo_hook)
const repo = new github.Repository(
  'inverter-desktop',
  {
    name: 'inverter-desktop',
    description:
      'Desktop version of Web dashboard for Victron inverter control with Home Assistant integration',
    visibility: 'public',
    hasIssues: true,
    hasWiki: true,
    hasProjects: true,
    allowMergeCommit: true,
    allowSquashMerge: true,
    allowRebaseMerge: true,
    allowAutoMerge: true, // current state: enabled
    deleteBranchOnMerge: true,
  },
  {
    // Import the existing repository – Pulumi will not recreate it.
    import: 'inverter-desktop',
    // Guard against accidental destroy of the physical repository.
    protect: true,
    // vulnerability_alerts is deprecated in the Repository resource; we
    // manage it separately with RepositoryVulnerabilityAlerts below.
    ignoreChanges: ['vulnerabilityAlerts'],
  }
)

// Manage Dependabot / vulnerability alerts as a dedicated resource.
const vulnAlerts = new github.RepositoryVulnerabilityAlerts('vulnerability-alerts', {
  repository: repo.name,
})

// ---------------------------------------------------------------------------
// Branch protection – main
// ---------------------------------------------------------------------------

// Require all CI jobs to pass before merging into main and enforce at least
// one approving review with stale-review dismissal.
new github.BranchProtection('main-protection', {
  repositoryId: repo.nodeId,
  pattern: 'main',
  // All commits pushed to main (including via PR merge) must be GPG/SSH signed.
  requireSignedCommits: true,
  // Ensure the branch is up-to-date with base before merging.
  requireConversationResolution: true,
  requiredStatusChecks: [
    {
      strict: true,
      contexts: [
        // Jobs defined in .github/workflows/ci.yml
        'frontend',
        'rust',
        // Job defined in .github/workflows/unit-tests.yml
        'vitest',
        // Job defined in .github/workflows/cargo-audit.yml
        'cargo-audit',
      ],
    },
  ],
  requiredPullRequestReviews: [
    {
      dismissStaleReviews: true,
      requireCodeOwnerReviews: false,
      requiredApprovingReviewCount: 1,
    },
  ],
})

// ---------------------------------------------------------------------------
// Actions secrets
// ---------------------------------------------------------------------------

// CARGO_REGISTRY_TOKEN is used by publish.yml to publish the crate to crates.io.
// Set this before deploying:
//   pulumi config set --secret cargoRegistryToken <your-crates.io-token>
const cargoRegistryToken = cfg.requireSecret('cargoRegistryToken')

const cargoSecret = new github.ActionsSecret('cargo-registry-token', {
  repository: repo.name,
  secretName: 'CARGO_REGISTRY_TOKEN',
  // plaintextValue is deprecated; use `value` (added in provider v6).
  value: cargoRegistryToken,
})

// ---------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------

export const repositoryUrl = repo.htmlUrl
export const repositoryName = repo.name
export const defaultBranch = pulumi.output('main')
