#!/usr/bin/env bash
# Fail if a crate that handles audio or transcripts can reach the network.
#
# OpenVoice promises local-first operation with no telemetry. This turns that
# promise into a build failure rather than a sentence in a README that quietly
# stops being true the first time someone adds a convenient dependency.
#
# ---------------------------------------------------------------------------
# What this proves, and what it does not
#
# Two guarantees, kept separate because they are not equally strong and
# pretending otherwise would be the exact rot this script exists to prevent:
#
#   SEALED      No path to an HTTP client, TLS stack or socket library anywhere
#               in the crate's transitive graph, build scripts included. These
#               are the crates that touch the microphone, the transcript, the
#               keyboard and the history database. Nothing in them can phone
#               home, because nothing in them can open a socket.
#
#   NO_DIRECT   No *direct* dependency on a network crate. `ov-app` is the Tauri
#               shell, and Tauri depends on reqwest unconditionally (asset
#               protocol, remote-URL windows), so an HTTP client is linked into
#               the shipped binary and this script cannot honestly claim
#               otherwise. What it does enforce is that no OpenVoice code
#               reaches for the network itself: telemetry, an update ping or a
#               crash uploader would have to appear as a direct dependency, and
#               that fails here.
#
# When ov-asr grows the model downloader it moves to NO_DIRECT with reqwest
# allowed, and that download becomes the only outbound request we make.
# ---------------------------------------------------------------------------

set -euo pipefail

# No path to the network at all.
SEALED_CRATES=(ov-core ov-format ov-audio ov-input ov-store ov-cli ov-asr)

# May contain a network client transitively; must not name one itself.
NO_DIRECT_CRATES=(ov-app)

# Substrings that indicate network capability.
FORBIDDEN='reqwest|hyper|ureq|isahc|curl|surf|tokio-tungstenite|rustls|native-tls|openssl|socket2'

status=0

in_workspace() {
  cargo metadata --no-deps --format-version 1 | grep -q "\"name\":\"$1\""
}

# --target all matters. Without it cargo tree resolves the host platform only,
# so a network dependency added under [target.'cfg(windows)'.dependencies] would
# be invisible to this job, which runs on Linux.
for crate in "${SEALED_CRATES[@]}"; do
  if ! in_workspace "${crate}"; then
    echo "skip ${crate} (not in workspace yet)"
    continue
  fi

  # A build script that phones home is as much of a violation as the library
  # doing it, hence --edges normal,build.
  hits=$(cargo tree -p "${crate}" --edges normal,build --prefix none --target all 2>/dev/null \
    | grep -Ei "^(${FORBIDDEN})" | sort -u || true)

  if [[ -n "${hits}" ]]; then
    echo "FAIL: ${crate} is sealed but has a path to the network:"
    echo "${hits}" | sed 's/^/    /'
    status=1
  else
    echo "sealed ok:     ${crate}"
  fi
done

for crate in "${NO_DIRECT_CRATES[@]}"; do
  if ! in_workspace "${crate}"; then
    echo "skip ${crate} (not in workspace yet)"
    continue
  fi

  # --depth 1 is the crate's own dependency list: what its Cargo.toml asks for.
  hits=$(cargo tree -p "${crate}" --depth 1 --edges normal,build --prefix none --target all 2>/dev/null \
    | grep -Ei "^(${FORBIDDEN})" | sort -u || true)

  if [[ -n "${hits}" ]]; then
    echo "FAIL: ${crate} takes a direct dependency on a network client:"
    echo "${hits}" | sed 's/^/    /'
    status=1
  else
    echo "no-direct ok:  ${crate}"
  fi
done

if [[ ${status} -ne 0 ]]; then
  cat <<'EOF'

A crate gained network capability it is not allowed to have.

Do not move the crate to a weaker list to make the build pass. Either:
  1. Move the code that needs the network into ov-asr's model downloader, or
  2. Write an ADR explaining why the local-first guarantee should change, and
     update SECURITY.md and the README's privacy section in the same PR.

EOF
fi

exit "${status}"
