---
name: github
description: Full GitHub CLI control — issues, PRs, code reviews, repo management. Uses gh CLI with auth detection, rate limiting, and templates. Triggers on: github, issue, pull request, PR, code review, repo, branch, label, assignee, milestone, release, workflow, actions.
---

# GitHub Skill

Full GitHub operations via the `gh` CLI. Always check auth first, use templates for consistency, and respect rate limits.

## Step 0: Auth Detection (ALWAYS FIRST)

Before ANY GitHub operation, detect the auth method:

```bash
gh auth status 2>&1
```

**If authenticated:** Proceed. Note the logged-in username for the session.

**If not authenticated:** Check for `GITHUB_TOKEN` env var:
```bash
echo "${GITHUB_TOKEN:+token_set}"
```

**If no token either:** Tell the user:
> GitHub CLI is not authenticated. Run `gh auth login` to authenticate, or set `GITHUB_TOKEN` environment variable with a personal access token.

Do NOT proceed without auth. Do NOT fall back to `curl` or `http_request`.

## Step 1: Rate Limit Check (before bulk operations)

Before operations that issue 5+ API calls (batch triage, bulk label, multi-issue create), check rate limits:

```bash
gh api rate_limit --jq '.resources.core.remaining, .resources.core.limit, .resources.search.remaining, .resources.search.limit'
```

If core remaining < 100 or search remaining < 10, warn the user and ask whether to proceed.

## Issue Operations

### Create Issue

```bash
gh issue create --repo OWNER/REPO --title "TITLE" --body "BODY" --label "LABEL" --assignee "USER"
```

Templates (use unless user provides their own):

**Bug report:**
```markdown
## Description

[What happened]

## Steps to Reproduce

1. [Step 1]
2. [Step 2]

## Expected Behavior

[What should happen]

## Actual Behavior

[What actually happened]

## Environment

- OS: [e.g., macOS 14.3]
- Version: [e.g., v0.3.70]
```

**Feature request:**
```markdown
## Problem

[What problem does this solve?]

## Proposed Solution

[How should it work?]

## Alternatives Considered

[What else was considered?]

## Additional Context

[Screenshots, logs, references]
```

### List Issues

```bash
gh issue list --repo OWNER/REPO --state open --limit 20
gh issue list --repo OWNER/REPO --label "bug" --state open
gh issue list --repo OWNER/REPO --assignee "@me"
```

### View Issue

```bash
gh issue view NUMBER --repo OWNER/REPO
gh issue view NUMBER --repo OWNER/REPO --comments
```

### Comment on Issue

```bash
gh issue comment NUMBER --repo OWNER/REPO --body "COMMENT"
```

### Close Issue

```bash
gh issue close NUMBER --repo OWNER/REPO --reason completed
gh issue close NUMBER --repo OWNER/REPO --comment "Closing because REASON"
```

### Label Issue

```bash
gh issue edit NUMBER --repo OWNER/REPO --add-label "bug,priority:high"
gh issue edit NUMBER --repo OWNER/REPO --remove-label "wontfix"
```

### Assign Issue

```bash
gh issue edit NUMBER --repo OWNER/REPO --add-assignee "username"
gh issue edit NUMBER --repo OWNER/REPO --remove-assignee "username"
```

## Pull Request Operations

### Create PR

```bash
gh pr create --repo OWNER/REPO --title "TITLE" --body "BODY" --base main --head feature-branch
```

PR template:
```markdown
## What

[One-line summary]

## Why

[Why this change is needed]

## How

[What changed and how it works]

## Testing

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-features -- -D warnings`
- [ ] `cargo test --all-features`

## Related Issues

Closes #NUMBER
```

### List PRs

```bash
gh pr list --repo OWNER/REPO --state open
gh pr list --repo OWNER/REPO --author "@me" --state open
```

### View PR

```bash
gh pr view NUMBER --repo OWNER/REPO
gh pr view NUMBER --repo OWNER/REPO --comments
gh pr diff NUMBER --repo OWNER/REPO
```

### Review PR

```bash
gh pr review NUMBER --repo OWNER/REPO --approve
gh pr review NUMBER --repo OWNER/REPO --request-changes --body "Please fix X"
gh pr review NUMBER --repo OWNER/REPO --comment --body "Looks good, one nit"
```

### Merge PR

```bash
gh pr merge NUMBER --repo OWNER/REPO --squash
gh pr merge NUMBER --repo OWNER/REPO --merge
gh pr merge NUMBER --repo OWNER/REPO --rebase
```

### Branch Operations

```bash
gh pr checkout NUMBER --repo OWNER/REPO
git push origin --delete feature-branch  # after merge
```

## Repository Operations

### View Repo

```bash
gh repo view OWNER/REPO
gh repo view OWNER/REPO --json name,description,defaultBranchRef,stargazerCount
```

### Clone

```bash
gh repo clone OWNER/REPO
gh repo clone OWNER/REPO /path/to/dir
```

### Fork

```bash
gh repo fork OWNER/REPO
gh repo fork OWNER/REPO --clone=false
```

### Releases

```bash
gh release list --repo OWNER/REPO
gh release view TAG --repo OWNER/REPO
gh release create TAG --repo OWNER/REPO --title "TITLE" --notes "NOTES"
```

## Workflow / Actions

```bash
gh workflow list --repo OWNER/REPO
gh run list --repo OWNER/REPO --limit 5
gh run view RUN_ID --repo OWNER/REPO
gh run watch RUN_ID --repo OWNER/REPO
```

## Code Search

```bash
gh search code "QUERY" --repo OWNER/REPO
gh search issues "QUERY" --repo OWNER/REPO
gh search prs "QUERY" --repo OWNER/REPO
```

## Rules

1. **ALWAYS use `gh` CLI, NEVER `http_request` to api.github.com** — http_request lacks proper headers and gets 403
2. **ALWAYS check auth before first operation** — Step 0 above
3. **ALWAYS use `--repo OWNER/REPO`** — don't rely on working directory detection
4. **NEVER expose tokens** — don't echo GITHUB_TOKEN, don't log it, don't include in bodies
5. **Batch operations check rate limits first** — Step 1 above
6. **Use `--jq` for filtering** — `gh api ... --jq '.field'` to extract specific values
7. **Templates are suggestions** — adapt to the user's actual content, don't force rigid structure
8. **Cross-reference usernames** — when the issue/comment mentions a user, check if they're in USER.md
