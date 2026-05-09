#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

# Render the /top demo gif. vhs's native gif quantizer produces visible
# stuttering on the long workload section; using vhs's mp4 as the source
# and re-encoding through ffmpeg's palette pipeline gives a much smoother
# result at the cost of file size (~11 MiB vs ~3 MiB).
#
# Usage:
#   bash demos/render-top.sh
#
# Output: demos/top-demo.gif (final), demos/top-demo.mp4 (intermediate,
# not committed to the repo).

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PALETTE="$(mktemp -t top-demo-palette.XXXXXX.png)"
readonly REPO_ROOT
readonly TAPE="${REPO_ROOT}/demos/top-demo.tape"
readonly GIF="${REPO_ROOT}/demos/top-demo.gif"
readonly MP4="${REPO_ROOT}/demos/top-demo.mp4"
readonly PALETTE

cleanup() {
  rm -f "${PALETTE}"
  rm -f "${MP4}"
}
trap cleanup EXIT

main() {
  pkill -f top-workload 2>/dev/null || true
  sleep 1

  # vhs reads the tape; the tape declares both gif + mp4 outputs.
  vhs "${TAPE}"

  # Re-encode the mp4 through ffmpeg's palette pipeline (15 fps, 1200 px
  # wide, bayer dither) and overwrite the vhs gif with the smoother one.
  ffmpeg -y -i "${MP4}" \
    -vf "fps=15,scale=1200:-1:flags=lanczos,palettegen=stats_mode=diff" \
    "${PALETTE}"

  ffmpeg -y -i "${MP4}" -i "${PALETTE}" \
    -lavfi "fps=15,scale=1200:-1:flags=lanczos[x];\
[x][1:v]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
    "${GIF}"

  printf "Wrote %s (%s bytes)\n" "${GIF}" \
    "$(stat -f '%z' "${GIF}" 2>/dev/null || stat -c '%s' "${GIF}")"
}

main "$@"
