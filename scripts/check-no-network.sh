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
#               in the crate's transitive graph. These are the crates that touch
#               the microphone, the transcript, the keyboard and the history
#               database. Nothing in them can phone home at run time, because
#               nothing in them can open a socket.
#
#               Checked over `--edges normal` -- what is linked into the shipped
#               binary -- and separately over `--edges build`, where a small
#               allow-list applies. See BUILD_FETCH below.
#
#   NO_DIRECT   No *direct* dependency on a network crate, except the ones named
#               in ALLOWED_DIRECT below. `ov-app` is the Tauri shell, and Tauri
#               depends on reqwest unconditionally (asset protocol, remote-URL
#               windows), so an HTTP client is linked into the shipped binary and
#               this script cannot honestly claim otherwise. What it does enforce
#               is that no OpenVoice code reaches for the network itself:
#               telemetry or a crash uploader would have to appear as a direct
#               dependency, and that fails here.
#
# ---------------------------------------------------------------------------
# The allowance, and why it is a list rather than a hole
#
# `tauri-plugin-updater` is a direct dependency of ov-app as of 2026-08-09, and
# it exists to make a request nobody individually asked for. That is a real
# weakening of the guarantee and it is recorded in ADR 0005.
#
# It is spelled out here rather than left to slip through, because it would have:
# the FORBIDDEN pattern below matches HTTP and TLS crates by name, and a Tauri
# plugin that wraps them matches none of those words. Adding it would have
# changed nothing in this script's output, which is exactly the silent drift the
# script exists to prevent. So network-capable dependencies of ov-app are now an
# explicit allow-list: anything not on it fails, including a future plugin that
# happens to be built on the same transport.
#
# `ov-fetch` joins that list as of 2026-09-04 (ADR 0009). It downloads speech
# models, which needs a socket, and it exists as a separate crate precisely so
# that the socket lives somewhere named rather than inside `ov-asr` — the crate
# that holds the microphone. It is matched by FORBIDDEN below like any other
# network client, so it cannot appear anywhere by accident.
#
# Adding an entry here means writing an ADR. That is the point of the friction.
#
# ---------------------------------------------------------------------------
# Build-time fetching, which is a different question from run-time capability
#
# `sherpa-onnx-sys` downloads prebuilt static libraries from k2-fsa's GitHub
# releases in its build script. That is a real supply-chain fact and it is why
# BUILD_FETCH exists rather than the build edges simply being ignored: a build
# script that phones home is still a build script that phones home, and it
# deserves to be named.
#
# What it is not is run-time capability. The libraries it fetches are linked
# statically; nothing network-capable ends up in the shipped binary, which is
# what `--edges normal` now proves separately. A user running OpenVoice cannot
# be reached by any of this. See ADR 0008.
# ---------------------------------------------------------------------------

set -euo pipefail

# No path to the network at all.
SEALED_CRATES=(ov-core ov-format ov-audio ov-input ov-store ov-cli ov-asr)

# May contain a network client transitively; must not name one itself, except as
# permitted by ALLOWED_DIRECT.
NO_DIRECT_CRATES=(ov-app)

# Crates permitted to reach the network *during compilation only*, one per line
# with the reason. Anything not listed here still fails the build-edge check.
BUILD_FETCH='sherpa-onnx-sys'   # ADR 0008 - fetches prebuilt sherpa-onnx libs, linked statically

# Crates that must never gain a *run-time* path to ov-fetch, checked explicitly.
#
# ov-asr is sealed already, so this is belt and braces -- but it is the crate
# holding the microphone, and "ov-asr cannot phone home" is the single claim in
# this file most worth being unable to break by accident. It takes ov-fetch as a
# dev-dependency for one ignored end-to-end test, which `--edges normal` does not
# see; this asserts that distinction still holds.
NEVER_FETCH=(ov-asr ov-core ov-format ov-audio ov-input ov-store)

# Substrings that indicate network capability.
# `ov-fetch` is ours and is listed deliberately: it wraps ureq, so without a
# name of its own here it could be added to a sealed crate and match none of the
# other words. That is the exact silent drift this script exists to prevent.
FORBIDDEN='reqwest|hyper|ureq|isahc|curl|surf|tokio-tungstenite|rustls|native-tls|openssl|socket2|tauri-plugin-updater|tauri-plugin-http|ov-fetch'

# Direct dependencies of a NO_DIRECT crate that are permitted anyway, one per
# line, each with the ADR that justifies it.
ALLOWED_DIRECT='tauri-plugin-updater
ov-fetch'
# tauri-plugin-updater  — ADR 0005: check-only, disableable, signed
# ov-fetch              — ADR 0009: model downloads the user starts, checksum-verified

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

  # Run time first: what is actually linked into the binary a user runs. This is
  # the guarantee that matters to them, and it admits no exceptions at all.
  #
  # A resolution failure used to be swallowed by `2>/dev/null || true`, leaving
  # `hits` empty and printing "sealed ok" for a check that never ran. That fired
  # for real: the 0.5.0 bump left the internal version pins at ^0.4.2, every
  # cargo invocation failed, and this script passed every crate. A safety net
  # that reports success when it is broken is worse than no net, so it is fatal.
  if ! tree=$(cargo tree -p "${crate}" --edges normal --prefix none --target all 2>&1); then
    echo "FAIL: cannot resolve the dependency tree for ${crate}:"
    echo "${tree}" | sed 's/^/    /'
    status=1
    continue
  fi
  hits=$(grep -Ei "^(${FORBIDDEN})" <<<"${tree}" | sort -u || true)

  if [[ -n "${hits}" ]]; then
    echo "FAIL: ${crate} links a network client into the binary:"
    echo "${hits}" | sed 's/^/    /'
    status=1
    continue
  fi

  # Build time: a different question. Permitted only for crates in BUILD_FETCH.
  #
  # `--edges build` alone is NOT enough: it shows only the root package's own
  # build-dependencies, so a build script three crates down the normal graph --
  # which is exactly where sherpa-onnx-sys sits -- is invisible to it. The full
  # graph is `normal,build`, and anything in it that was not already in the
  # run-time graph got there through some build script.
  if ! btree=$(cargo tree -p "${crate}" --edges normal,build --prefix none --target all 2>&1); then
    echo "FAIL: cannot resolve the build tree for ${crate}:"
    echo "${btree}" | sed 's/^/    /'
    status=1
    continue
  fi
  bhits=$(grep -Eio "^(${FORBIDDEN})" <<<"${btree}" | sort -u || true)

  if [[ -z "${bhits}" ]]; then
    echo "sealed ok:     ${crate}"
    continue
  fi

  # Every hit here is build-introduced: the run-time graph above was clean. Ask
  # cargo which crate pulls each one in, and accept it only if that chain passes
  # through something named in BUILD_FETCH. An empty BUILD_FETCH therefore
  # explains nothing and fails, which is the behaviour you want from a list whose
  # whole job is to be short.
  unexplained=""
  for hit in ${bhits}; do
    [[ -z "${hit}" ]] && continue
    chain=$(cargo tree -p "${crate}" --edges normal,build --prefix none --target all \
      --invert "${hit}" 2>/dev/null | grep -Eo '^[a-z0-9_+.-]+' | sort -u || true)
    ok=0
    while IFS= read -r allowed; do
      allowed=${allowed%%#*}
      allowed=$(echo "${allowed}" | xargs)
      [[ -z "${allowed}" ]] && continue
      if grep -qx -- "${allowed}" <<<"${chain}"; then ok=1; fi
    done <<<"${BUILD_FETCH}"
    [[ ${ok} -eq 0 ]] && unexplained+="${hit}"$'\n'
  done

  if [[ -n "${unexplained//[$'\n' ]/}" ]]; then
    echo "FAIL: ${crate} reaches the network at build time via an unlisted crate:"
    echo "${unexplained}" | sed '/^$/d; s/^/    /'
    status=1
    continue
  fi
  echo "sealed ok:     ${crate} (build-time fetch only, see ADR 0008)"
done

for crate in "${NO_DIRECT_CRATES[@]}"; do
  if ! in_workspace "${crate}"; then
    echo "skip ${crate} (not in workspace yet)"
    continue
  fi

  # --depth 1 is the crate's own dependency list: what its Cargo.toml asks for.
  hits=$(cargo tree -p "${crate}" --depth 1 --edges normal,build --prefix none --target all 2>/dev/null \
    | grep -Ei "^(${FORBIDDEN})" | sort -u || true)

  # Drop the allowed entries. Matched on the crate name alone -- `cargo tree`
  # prints "name vX.Y.Z", and comparing the whole line would make this silently
  # stop matching on the next version bump.
  unexpected=""
  while IFS= read -r line; do
    [[ -z "${line}" ]] && continue
    name=${line%% *}
    if ! grep -qx -- "${name}" <<<"${ALLOWED_DIRECT}"; then
      unexpected+="${line}"$'\n'
    fi
  done <<<"${hits}"

  if [[ -n "${unexpected//[$'\n' ]/}" ]]; then
    echo "FAIL: ${crate} takes a direct dependency on a network client:"
    echo "${unexpected}" | sed '/^$/d; s/^/    /'
    status=1
  else
    echo "no-direct ok:  ${crate}"
    while IFS= read -r allowed; do
      [[ -z "${allowed}" ]] && continue
      if grep -qi "^${allowed} " <<<"${hits}"; then
        echo "  allowed:     ${allowed} (justified in ALLOWED_DIRECT above)"
      fi
    done <<<"${ALLOWED_DIRECT}"
  fi
done

# ---------------------------------------------------------------------------
for crate in "${NEVER_FETCH[@]}"; do
  in_workspace "${crate}" || continue
  if ! tree=$(cargo tree -p "${crate}" --edges normal --prefix none --target all 2>&1); then
    echo "FAIL: cannot resolve the run-time tree for ${crate}"
    status=1
    continue
  fi
  if grep -qEi '^ov-fetch' <<<"${tree}"; then
    echo "FAIL: ${crate} has a run-time dependency on ov-fetch."
    echo "    A dev-dependency is fine; a normal one is not. ${crate} must stay sealed."
    status=1
  else
    echo "no-fetch ok:   ${crate}"
  fi
done

if [[ ${status} -ne 0 ]]; then
  cat <<'EOF'

A crate gained network capability it is not allowed to have.

Do not move the crate to a weaker list to make the build pass. Either:
  1. Move the code that needs the network into `ov-fetch`, which exists for
     exactly this and is the only crate allowed to open a socket, or
  2. Write an ADR explaining why the local-first guarantee should change, and
     update SECURITY.md and the README's privacy section in the same PR.

EOF
fi

exit "${status}"
