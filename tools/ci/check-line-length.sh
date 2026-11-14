#!/usr/bin/env bash
# No line of tracked Rust under crates/ may be longer than 200 bytes.
#
# `cargo fmt --check` does not catch these. rustfmt gives up on a chain or
# statement that holds something it cannot fit (a long string literal, a
# dense closure) and leaves the whole of it exactly as written, without a
# word; a 400-character line then passes the format check. Split such code
# into named helpers and `const` strings, wrap a long literal with `\` (or
# `concat!` for a raw string), and rustfmt formats the rest again.
#
# Lengths are counted in bytes (LC_ALL=C), so every awk gives the same answer.
set -euo pipefail
cd "$(dirname "$0")/../.."
limit=200

# Exemptions: "<path> <lines allowed over the limit>", each with its reason.
# A file may hold at most that many; one more fails. Keep this list short
# and remove an entry when its file no longer needs it.
allowed() {
  cat <<'EOF'
# A row of the Markdown table in the module's docs: a table row cannot be wrapped.
crates/iptv-providers/src/m3u/catchup/mod.rs 1
# One-line JSON, XML and playlist fixtures (and four dense test lines) in the
# network, import and provider crates. They can be wrapped like the rest were;
# they are left because those crates were being changed elsewhere when this
# check was added. Wrap them and drop these entries once that work has landed.
crates/iptv-import/tests/it/common/synth.rs 4
crates/iptv-import/tests/it/parser_properties.rs 4
crates/iptv-net/src/playback/dash.rs 1
crates/iptv-net/tests/it/playback.rs 1
crates/iptv-providers/src/m3u/tests.rs 1
crates/iptv-providers/tests/it/stalker.rs 1
EOF
}

long=$(git ls-files -z 'crates/**/*.rs' |
  LC_ALL=C xargs -0 awk -v limit="$limit" 'length($0) > limit { print FILENAME ":" FNR ": " length($0) }')

# One awk over both lists: "A <path> <count>" allows, "L <path>:<line>: <length>" is a long line.
failed=0
report=$(
  {
    allowed | grep -v '^#' | sed 's/^/A /'
    [ -z "$long" ] || printf '%s\n' "$long" | sed 's/^/L /'
  } | LC_ALL=C awk -v limit="$limit" '
    $1 == "A" { allow[$2] = $3; next }
    {
      row = substr($0, 3)
      split(row, part, ":")
      count[part[1]]++
      lines[part[1]] = lines[part[1]] "  " row "\n"
    }
    END {
      for (file in count) {
        may = (file in allow) ? allow[file] + 0 : 0
        if (count[file] > may) {
          bad = 1
          printf "%s: %d line(s) over %d bytes (%d allowed):\n%s", file, count[file], limit, may, lines[file]
        }
      }
      for (file in allow) {
        if (count[file] + 0 < allow[file]) {
          printf "note: %s now has %d long line(s); lower its entry in tools/ci/check-line-length.sh\n", file, count[file] + 0
        }
      }
      exit bad
    }'
) || failed=1

[ -z "$report" ] || printf '%s\n' "$report"
if [ "$failed" -ne 0 ]; then
  echo "Lines over $limit bytes: rustfmt leaves such code unformatted. Split it up (see the head of this script)." >&2
  exit 1
fi
