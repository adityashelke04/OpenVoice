#!/usr/bin/env bash
# Every advertised download link must be one that cannot go stale.
#
# # Why this exists
#
# The README's download button read
# `.../releases/download/v0.4.2/OpenVoice_0.4.2_x64-setup.exe`. There was no
# v0.4.2 release; there was no v0.4.2 tag. The link 404'd, and someone who wanted
# the app did the reasonable thing -- went to the releases page and took the
# newest installer they could see, which was v0.4.0, carrying every bug the two
# unshipped releases had fixed. The project's own front door handed out a build
# it had already replaced twice.
#
# The cause is structural, not careless: a version number written by hand into a
# URL is a claim that nothing checks. `release.yml` publishes every installer to a
# fixed `download` tag under a fixed filename precisely so that no link ever needs
# a version in it, and this makes using anything else a build failure.
#
# Note the scope. This forbids *versioned installer* URLs in documentation. Links
# to the releases page, to a specific release's notes, or to the updater manifest
# are all fine -- none of them is a download button, and none of them goes stale.
set -euo pipefail

readonly STABLE='releases/download/download/OpenVoice-x64-setup.exe'

# Documentation a user might follow. Deliberately not the whole tree: CHANGELOG
# entries name the release they describe, and rewriting history to satisfy a
# linter would be worse than the problem.
files=(README.md)
while IFS= read -r -d '' f; do files+=("$f"); done < <(find docs -name '*.md' -print0 2>/dev/null)

fail=0

for f in "${files[@]}"; do
  [[ -f "$f" ]] || continue
  # A versioned installer URL: `/releases/download/<tag>/<something>setup.exe`
  # where <tag> is anything but the literal `download` tag the workflow keeps
  # current. `grep -P` for the negative lookahead; mixing -P with -E silently
  # matches nothing, which is a check that always passes.
  hits=$(grep -nP 'releases/download/(?!download/)[^/ ]+/[^) ]*setup\.exe' "$f" || true)
  if [[ -n "$hits" ]]; then
    echo "error: $f advertises a version-pinned installer URL."
    echo "$hits" | sed 's/^/    /'
    echo "    Use the permanent link instead, which release.yml keeps current:"
    echo "        https://github.com/adityashelke04/OpenVoice/$STABLE"
    echo
    fail=1
  fi
done

# The README must actually carry the permanent link. A file that simply dropped
# its download button would otherwise pass the check above by saying nothing.
if ! grep -qF "$STABLE" README.md; then
  echo "error: README.md no longer contains the permanent download link."
  echo "    Expected a link to https://github.com/adityashelke04/OpenVoice/$STABLE"
  fail=1
fi

if [[ $fail -ne 0 ]]; then
  exit 1
fi

echo "download links are version-free and point at the permanent installer URL"
