#!/bin/sh
# The frame-src of the console's Content-Security-Policy (nginx.conf includes what this writes):
# the origin of NEXC_ONTO_PUBLIC_URL, where browsers reach onto-intelli's UI that the workspace
# map embeds, or 'none' without one. The nginx image's entrypoint runs it before nginx starts;
# /tmp is the only place it may write (the root filesystem is read-only).
set -eu

dir=/tmp/nginx
mkdir -p "$dir"
src="'none'"
url="${NEXC_ONTO_PUBLIC_URL:-}"
if [ -n "$url" ]; then
  # scheme://host[:port] only: the origin the frame is loaded from, never a wildcard.
  origin="$(printf '%s' "$url" | sed -E 's|^(https?://[^/?#]+).*$|\1|')"
  if printf '%s' "$origin" | grep -Eq '^https?://(\[[0-9A-Fa-f:.]+\]|[A-Za-z0-9.-]+)(:[0-9]{1,5})?$'; then
    src="$origin"
  else
    echo "40-nexc-frame-src.sh: NEXC_ONTO_PUBLIC_URL is not an http(s) URL; the workspace map stays blocked" >&2
  fi
fi
# shellcheck disable=SC2016 # an nginx variable, written as it is
printf 'set $onto_frame_src "%s";\n' "$src" >"$dir/frame-src.conf"
echo "40-nexc-frame-src.sh: frame-src $src"
