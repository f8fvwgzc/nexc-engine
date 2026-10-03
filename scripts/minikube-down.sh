#!/usr/bin/env bash
# Remove nexc-engine from minikube. Pass --delete to delete the whole minikube profile as well.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PROFILE="${MINIKUBE_PROFILE:-nexc}"

if ! minikube -p "$PROFILE" status >/dev/null 2>&1; then
  echo "minikube profile '$PROFILE' is not running"
  exit 0
fi
kubectl config use-context "$PROFILE" >/dev/null

if [[ -f deploy/k8s/secret.env ]]; then
  kubectl delete -k deploy/k8s --ignore-not-found --wait=true
else
  kubectl delete namespace nexc --ignore-not-found --wait=true
fi

if [[ "${1:-}" == "--delete" ]]; then
  minikube -p "$PROFILE" delete
else
  echo "namespace removed; minikube keeps running (stop: minikube -p $PROFILE stop," \
       "delete: scripts/minikube-down.sh --delete)"
fi
