#!/usr/bin/env bash
# Fail if any crate outside the model downloader can reach the network.
#
# OpenVoice promises local-first operation with no telemetry. This turns that
# promise into a build failure rather than a sentence in a README that quietly
# stops being true the first time someone adds a convenient dependency.
#
# The only legitimate outbound request in the entire application is an explicit,
# user-initiated model download, which lives in ov-asr.

set -euo pipefail

# Crates that must have no path to an HTTP client, TLS stack, or socket library.
SEALED_CRATES=(ov-core ov-format ov-audio ov-input ov-store)

# Substrings that indicate network capability.
FORBIDDEN='reqwest|hyper|ureq|isahc|curl|surf|tokio-tungstenite|rustls|native-tls|openssl|socket2'

status=0

for crate in "${SEALED_CRATES[@]}"; do
  if ! cargo metadata --no-deps --format-version 1 \
      | grep -q "\"name\":\"${crate}\""; then
    echo "skip ${crate} (not in workspace yet)"
    continue
  fi

  # cargo tree over the crate's full transitive graph, including build deps:
  # a build script that phones home would be just as much of a violation.
  hits=$(cargo tree -p "${crate}" --edges normal,build --prefix none 2>/dev/null \
    | grep -Ei "^(${FORBIDDEN})" || true)

  if [[ -n "${hits}" ]]; then
    echo "FAIL: ${crate} has a path to the network:"
    echo "${hits}" | sed 's/^/    /'
    status=1
  else
    echo "ok:   ${crate}"
  fi
done

if [[ ${status} -ne 0 ]]; then
  cat <<'EOF'

A sealed crate gained network capability.

Do not add this crate to the allowlist to make the build pass. Either:
  1. Move the code that needs the network into ov-asr's model downloader, or
  2. Write an ADR explaining why the local-first guarantee should change.

EOF
fi

exit "${status}"
