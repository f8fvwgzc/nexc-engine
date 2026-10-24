#!/bin/sh
# The HTTPS listener (HTTP/2, and HTTP/3 over QUIC): on when /etc/nginx/tls holds tls.crt and
# tls.key, off otherwise (nginx.conf includes what this writes; an empty file is no listener).
# NEXC_HTTPS_PUBLIC_PORT is the port browsers reach it on, advertised in Alt-Svc (443 by
# default; Compose publishes 8443). The nginx image's entrypoint runs this before nginx starts;
# /tmp is the only place it may write (the root filesystem is read-only).
set -eu

dir=/tmp/nginx
mkdir -p "$dir"
out="$dir/tls-server.conf"
cert=/etc/nginx/tls/tls.crt
key=/etc/nginx/tls/tls.key

if [ ! -e "$cert" ] && [ ! -e "$key" ]; then
  : >"$out"
  echo "41-nexc-tls.sh: no certificate in /etc/nginx/tls; HTTPS and HTTP/3 are off"
  exit 0
fi
# Half a certificate, or one nginx may not read, is a mistake to report, not a reason to serve
# plain http only.
for file in "$cert" "$key"; do
  if [ ! -r "$file" ]; then
    echo "41-nexc-tls.sh: $file is missing or not readable by uid $(id -u)" >&2
    exit 1
  fi
done

port="${NEXC_HTTPS_PUBLIC_PORT:-443}"
case "$port" in
  '' | *[!0-9]*)
    echo "41-nexc-tls.sh: NEXC_HTTPS_PUBLIC_PORT is not a port: $port" >&2
    exit 1
    ;;
esac
sed "s/__PUBLIC_PORT__/$port/" /etc/nginx/nexc/tls-server.conf.template >"$out"
echo "41-nexc-tls.sh: HTTPS with HTTP/2 and HTTP/3 on 8443, advertised as h3 on port $port"
