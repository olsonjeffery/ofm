# The list of vouched (or actively denounced) users for this repository.
#
# Only vouched users can open PRs here; unvouched/denounced PRs are
# auto-closed by .github/workflows/vouch-pr.yml (mitchellh/vouch check-pr).
# Issues are intentionally NOT gated here — robomp triages those.
#
# Write-access collaborators and bots are auto-allowed and need no entry.
# A denounced user ("-handle") is always blocked, even if also listed.
#
# Syntax:
#  - One handle per line (without @), sorted alphabetically.
#  - Optional platform prefix: `platform:username` (default platform: github).
#  - Denounce by prefixing with minus: `-username` / `-platform:username`.
#  - Optional free-text reason after a space following the handle.
#
# Maintainers manage this list by commenting `!vouch` / `!denounce [user]`
# on a discussion (see .github/workflows/vouch-manage.yml).
#
# Seed (2026-06-19): authors with >=2 merged PRs in the prior 6 months.
# Audit found 0 denounce-worthy actors; all reverts were maintainer
# technical rollbacks, not abuse. See the PR/commit history for provenance.
