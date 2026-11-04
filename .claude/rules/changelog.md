---
paths:
  - "CHANGELOG.md"
  - ".github/workflows/release*.yml"
  - ".github/workflows/mask-check.yml"
  - "dist-workspace.toml"
---

# CHANGELOG.md is the release body

## Release notes: state the fix, keep it short

**The top section of `CHANGELOG.md` *is* the GitHub release body.** cargo-dist parses the file and publishes that section as `announcement_github_body` — nothing in `dist-workspace.toml` names the file or reshapes the text. So it is written for someone deciding whether to upgrade, not for a reviewer reconstructing the work.

- **State the fix, not the problem.** "`sn cmdb create` and `sn cmdb update` work" — not "they sent a flat body, which that API cannot accept under any input: PATCH came back as a raw Java NPE…". A user upgrading does not need the defect's history, and leading with it buries the one sentence they came for.
- **One line per entry**, and keep the whole section **under ~4,000 characters**. 0.12.0 shipped at 37,114 and rendered as an unbroken wall; the rewrite says the same things in 3,383.
- **New commands lead**, and they are the one place to spend words: name the verb and describe what it now lets a caller do. Everything else follows — `### Breaking`, then `### Fixed`, then `### Security`. A release is read first for what it adds.
- **Design rationale, live measurements and rejected alternatives do not belong here.** They belong in this file and `docs/`, which is where they already are — the changelog was duplicating them. Keep a number only when the number is the user-facing result ("800 MB at 19 MB peak RSS"), not when it is evidence for a design choice.
- **Never use `## ` inside a release section.** `mask-check.yml` extracts the top section with `awk '/^## /{n++} n==1'`, so a nested `##` truncates what the preflight checks. Subsections are `### `.

## The changelog can silently empty a release

**Never write `Bearer <word>` in `CHANGELOG.md`.** It will publish a release with zero assets while reporting success.

cargo-dist passes the whole dist-manifest — release body included — as the `plan` job's `val` output, and every downstream job gates on `fromJson(needs.plan.outputs.val)`. The runner refuses to set an output containing a masked string, so `val` arrives empty, every `if` evaluates false, and build/host/publish all **skip**. Skipped is not failed, so the run is green. This emptied v0.8.0 three times.

The masked string is not one of your secrets. The runner registers CredScan credential regexes alongside real secrets, and one matches a bearer credential. Measured on the runner 2026-07-14 (see the `mask-forensics` workflow in git history):

| masked | not masked |
|---|---|
| `Bearer auth`, `Bearer xyzzy`, `Bearer 123`, `Bearer x` | `bearer auth` (case-sensitive), `BEARER AUTH` |
| `Bearer` ⏎ `auth` — **a newline is whitespace, so wrapping is no escape** | `Bearer` alone (needs a token after it) |
| `Authorization: Bearer abc123`, and the phrase anywhere inside prose | `Bearer-auth`, `Bearer.auth`, `Bearer_auth`, `Bearer, auth` (needs whitespace) |
| | `Bearers auth`, `MyBearer auth` (word boundaries required) |

So the rule is `\bBearer` + whitespace + any non-whitespace character. Say "bearer token" lowercase, or "OAuth access token", and rely on `mask-check` rather than on remembering this. **This was misdiagnosed for months** as junk prose lines pasted into `HOMEBREW_TAP_TOKEN`; that secret is a clean single line (`mask-check`'s `shape` job proves it every run) and rotating it changes nothing. A multiline secret *is* a real hazard of the same family — each line becomes a repo-wide mask — which is why `shape` still checks.
