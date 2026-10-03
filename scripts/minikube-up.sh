#!/usr/bin/env bash
# Start minikube, build the three images inside it and deploy nexc-engine with kustomize.
#   scripts/minikube-up.sh        (or: make minikube-up)
# Environment: MINIKUBE_PROFILE (default nexc), MINIKUBE_CPUS (4), MINIKUBE_MEMORY (8192),
#              MINIKUBE_DRIVER (minikube's default), MINIKUBE_CNI (calico: enforces NetworkPolicy)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PROFILE="${MINIKUBE_PROFILE:-nexc}"
NAMESPACE=nexc
HOST=nexc.local

for tool in minikube kubectl docker openssl; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required" >&2; exit 1; }
done

if ! minikube -p "$PROFILE" status >/dev/null 2>&1; then
  echo "==> starting minikube profile '$PROFILE'"
  args=(start -p "$PROFILE" --cpus="${MINIKUBE_CPUS:-4}" --memory="${MINIKUBE_MEMORY:-8192}"
        --cni="${MINIKUBE_CNI:-calico}")
  if [[ -n "${MINIKUBE_DRIVER:-}" ]]; then args+=(--driver="$MINIKUBE_DRIVER"); fi
  minikube "${args[@]}"
fi
kubectl config use-context "$PROFILE" >/dev/null

echo "==> enabling the ingress addon"
minikube -p "$PROFILE" addons enable ingress >/dev/null
kubectl -n ingress-nginx rollout status deployment/ingress-nginx-controller --timeout=300s

echo "==> building images inside minikube's docker daemon"
eval "$(minikube -p "$PROFILE" docker-env)"
docker build -t ghcr.io/f8fvwgzc/nexc-engine-backend:local backend
docker build -t ghcr.io/f8fvwgzc/nexc-engine-runtime:local agent-runtime
docker build -t ghcr.io/f8fvwgzc/nexc-engine-frontend:local frontend

echo "==> secrets"
scripts/init-env.sh --k8s

echo "==> deploying"
kubectl apply -k deploy/k8s
kubectl -n "$NAMESPACE" rollout status statefulset/postgres --timeout=300s
for deployment in agent-runtime backend frontend; do
  # Restart so freshly built `local` images are picked up on re-runs.
  kubectl -n "$NAMESPACE" rollout restart "deployment/$deployment" >/dev/null
  kubectl -n "$NAMESPACE" rollout status "deployment/$deployment" --timeout=600s
done

IP="$(minikube -p "$PROFILE" ip)"
cat <<MSG

nexc-engine is running in namespace '$NAMESPACE'.

  Linux (and VM drivers):   echo "$IP $HOST" | sudo tee -a /etc/hosts
  macOS/Windows + docker:   echo "127.0.0.1 $HOST" | sudo tee -a /etc/hosts
                            minikube -p $PROFILE tunnel     # keep this running

  Then open http://$HOST

  kubectl -n $NAMESPACE get pods
  kubectl -n $NAMESPACE logs deploy/backend -f
MSG
