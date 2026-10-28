---
id: ops-kubernetes-operations
name: Kubernetes operations
category: devops
description: Operates Kubernetes workloads and clusters: probes, requests and limits, autoscaling, RBAC, NetworkPolicy, Pod Security Standards, upgrades and etcd backup, aligned with the CKA and CKS outlines.
keywords: kubernetes, k8s, kubectl, pod, deployment, probes, requests and limits, hpa, autoscaling, rbac, networkpolicy, pod security standards, cluster upgrade, etcd, cka, cks
source: Kubernetes - Releases | https://kubernetes.io/releases/
source: Kubernetes - Version skew policy | https://kubernetes.io/releases/version-skew-policy/
source: Kubernetes - Pod Security Standards | https://kubernetes.io/docs/concepts/security/pod-security-standards/
source: Kubernetes - Horizontal Pod Autoscaling | https://kubernetes.io/docs/concepts/workloads/autoscaling/horizontal-pod-autoscale/
source: Linux Foundation - Certified Kubernetes Administrator (CKA) | https://training.linuxfoundation.org/certification/certified-kubernetes-administrator-cka/
---
## When to use
Writing or reviewing manifests, hardening clusters, troubleshooting pods, autoscaling and upgrades. Supported minors in October 2026: 1.37, 1.36, 1.35 (1.34 ends 2026-10-27), each patched for about 14 months. CKA (exam on v1.37) weights: troubleshooting 30%, cluster architecture/installation 25%, services and networking 20%, workloads and scheduling 15%, storage 10%. CKS (exam on v1.35, needs a passed CKA): cluster setup 15%, cluster hardening 15%, system hardening 10%, microservice vulnerabilities 20%, supply chain 20%, monitoring/logging/runtime 20%. Exam versions move; verify locally.

## Workload baseline
- Deployment (stateless), StatefulSet (stable identity and volumes), DaemonSet (per node), Job/CronJob (batch).
- Probes: startupProbe for slow boot; readinessProbe gates traffic; livenessProbe only detects a stuck process. Never check downstream dependencies in liveness, or one outage restarts every pod.
- Requests drive scheduling, limits cap use. QoS: Guaranteed (requests = limits), Burstable, BestEffort (evicted first). Memory limit = request is predictable; CPU limits throttle, use deliberately.
- Availability: 2+ replicas, PodDisruptionBudget, topologySpreadConstraints across zones, graceful shutdown (preStop, terminationGracePeriodSeconds).
- HPA: desired = ceil(current x currentMetric / targetMetric); 10% default tolerance, settable per HPA under behavior.scaleUp/scaleDown.tolerance (stable in 1.37); scale-down stabilisation 300 s, scale-up immediate (up to 100% or 4 pods per 15 s); utilisation targets need requests. Example: 4 pods at 90% CPU, target 60% gives ceil(6) = 6.

## Security
- RBAC: Role/RoleBinding per namespace, ClusterRole/ClusterRoleBinding cluster-wide; least privilege, no wildcards, no cluster-admin for CI; one ServiceAccount per app, automountServiceAccountToken false unless needed; check with kubectl auth can-i --as.
- NetworkPolicy: default-deny ingress and egress per namespace, then explicit allows (include DNS on 53); requires a CNI that enforces it.
- Pod Security Admission labels pod-security.kubernetes.io/enforce|audit|warn = privileged|baseline|restricted. Restricted needs runAsNonRoot, allowPrivilegeEscalation false, capabilities drop ALL (NET_BIND_SERVICE may be re-added), seccompProfile RuntimeDefault or Localhost, limited volume types. Use warn and audit before enforce.
- Encrypt Secrets at rest (EncryptionConfiguration, KMS v2), API audit logs, signed images checked at admission, runtime detection (e.g. Falco), minimal images, patched nodes, private API endpoint.

## Upgrades
- One minor at a time, latest patch first. Order: kube-apiserver, then controller-manager, scheduler and cloud-controller-manager (may trail by one minor), then kubelets (drain each node; no in-place minor upgrade), then kube-proxy. Kubelet and kube-proxy may trail the API server by up to three minors, never lead; kubectl within one minor either way.
- Before: check removed APIs, add-on and webhook compatibility; etcdctl snapshot save and a test restore; rehearse in staging.

## Troubleshooting order
get and describe (events), logs --previous, exit 137 (often OOMKilled), Pending (resources, taints, PVC), ImagePullBackOff, Service selector and EndpointSlices, DNS, NetworkPolicy, node conditions.

## Output
Findings table (resource, file and line, issue, severity, corrected YAML, verify command such as kubectl auth can-i or kubectl get events); upgrade plan with pre-checks, order, backup and rollback.

## Pitfalls
Missing requests; liveness on dependencies; mutable latest tags; skipping minors; cluster-admin everywhere.
