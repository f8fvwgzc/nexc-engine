#!/bin/sh
# The backend nginx proxies to, looked up again while nginx runs (nginx.conf includes what this
# writes in its upstream). A backend container that is recreated or scaled gets new addresses
# (Compose gives each one its own), and a name resolved only when nginx starts would keep every
# /api request on the old ones, 502s, until nginx restarted. nginx asks the name servers of
# /etc/resolv.conf itself: Docker's (127.0.0.11) under Compose, the cluster DNS in Kubernetes.
# Its resolver has no search list, so a bare name gets the pod's namespace domain in Kubernetes
# (backend -> backend.<namespace>.svc.<cluster domain>, the first search domain there).
# NEXC_BACKEND_UPSTREAM is the backend's host:port (backend:8080 by default). The nginx image's
# entrypoint runs this before nginx starts; /tmp is the only place it may write (the root
# filesystem is read-only).
set -eu

dir=/tmp/nginx
mkdir -p "$dir"
upstream="${NEXC_BACKEND_UPSTREAM:-backend:8080}"
host="${upstream%:*}"
port="${upstream##*:}"
if [ "$host" = "$upstream" ] || ! printf '%s' "$host" | grep -Eq '^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$'; then
  echo "39-nexc-backend.sh: NEXC_BACKEND_UPSTREAM is not host:port: $upstream" >&2
  exit 1
fi
case "$port" in
  '' | *[!0-9]*)
    echo "39-nexc-backend.sh: NEXC_BACKEND_UPSTREAM has no port: $upstream" >&2
    exit 1
    ;;
esac

# Every name server, IPv6 ones in brackets; one with a zone (fe80::1%eth0) nginx cannot use.
servers="$(awk '$1 == "nameserver" && $2 !~ /%/ { printf "%s ", ($2 ~ /:/ ? "[" $2 "]" : $2) }' /etc/resolv.conf)"
servers="${servers% }"

if printf '%s' "$host" | grep -Eq '^[0-9.]+$'; then
  # An address: nothing to look up.
  printf 'server %s:%s;\n' "$host" "$port" >"$dir/backend.conf"
  echo "39-nexc-backend.sh: backend $host:$port"
  exit 0
fi
if [ -z "$servers" ]; then
  echo "39-nexc-backend.sh: no name server in /etc/resolv.conf to look $host up with" >&2
  exit 1
fi
case "$host" in
  *.*) ;;
  *)
    domain="$(awk '$1 == "search" { print $2; exit }' /etc/resolv.conf)"
    case "$domain" in
      *.svc.*) host="$host.$domain" ;;
    esac
    ;;
esac
# valid=10s: asked again every 10 s, whatever TTL the answer carries (Docker's is 600 s). IPv4
# only: the backend listens on 0.0.0.0 (NEXC_HOST).
{
  printf 'resolver %s valid=10s ipv6=off;\n' "$servers"
  printf 'resolver_timeout 5s;\n'
  printf 'server %s:%s resolve;\n' "$host" "$port"
} >"$dir/backend.conf"
echo "39-nexc-backend.sh: backend $host:$port, looked up every 10 s through $servers"
