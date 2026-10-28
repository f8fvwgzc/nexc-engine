{{/* Labels every object carries. */}}
{{- define "nexc.labels" -}}
app.kubernetes.io/part-of: nexc-engine
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
{{- end }}

{{/* An image reference: `nexc.image (dict "image" .Values.images.backend "chart" .Chart)`. */}}
{{- define "nexc.image" -}}
{{ .image.repository }}:{{ .image.tag | default .chart.AppVersion }}
{{- end }}

{{/* The Secret the services read. */}}
{{- define "nexc.secretName" -}}
{{ .Values.secrets.existingSecret | default "nexc-secrets" }}
{{- end }}

{{/* The pod-level security context every workload shares (the restricted standard). */}}
{{- define "nexc.podSecurity" -}}
runAsNonRoot: true
runAsUser: {{ .uid }}
runAsGroup: {{ .uid }}
{{- if .fsGroup }}
fsGroup: {{ .uid }}
fsGroupChangePolicy: OnRootMismatch
{{- end }}
seccompProfile: { type: RuntimeDefault }
{{- end }}

{{/* The container-level security context every workload shares. */}}
{{- define "nexc.containerSecurity" -}}
allowPrivilegeEscalation: false
readOnlyRootFilesystem: true
capabilities: { drop: [ALL] }
{{- end }}

{{/* The ingress's TLS blocks: ingress.tls, else one of ingress.tlsSecretName for the chart's hosts. */}}
{{- define "nexc.ingressTls" -}}
{{- $i := .Values.ingress -}}
{{- if $i.tls -}}
{{ toYaml $i.tls }}
{{- else if $i.tlsSecretName -}}
{{- $hosts := list $i.host -}}
{{- if and .Values.ontoIntelli.enabled .Values.ontoIntelli.ingress.host -}}
{{- $hosts = append $hosts .Values.ontoIntelli.ingress.host -}}
{{- end -}}
{{ toYaml (list (dict "hosts" $hosts "secretName" $i.tlsSecretName)) }}
{{- end -}}
{{- end }}

{{/* Whether `host` is served over TLS by the chart's ingress: `nexc.tlsHost (dict "host" … "root" $)`. */}}
{{- define "nexc.tlsHost" -}}
{{- $tls := false }}
{{- range (include "nexc.ingressTls" .root | fromYamlArray) }}
{{- if has $.host (.hosts | default list) }}{{ $tls = true }}{{ end }}
{{- end }}
{{- if or $tls .root.Values.ingress.tlsUpstream }}true{{ end }}
{{- end }}

{{/* Where browsers reach the console: config.NEXC_PUBLIC_URL, else ingress.host (https with TLS). */}}
{{- define "nexc.publicUrl" -}}
{{- if .Values.config.NEXC_PUBLIC_URL -}}
{{ .Values.config.NEXC_PUBLIC_URL }}
{{- else if .Values.ingress.enabled -}}
http{{ if include "nexc.tlsHost" (dict "host" .Values.ingress.host "root" .) }}s{{ end }}://{{ .Values.ingress.host }}
{{- end -}}
{{- end }}

{{/*
The address of onto-intelli the backend calls: backend.ontoUrl, else the release's Service when
ontoIntelli.enabled; empty when onto-intelli is off.
*/}}
{{- define "nexc.ontoUrl" -}}
{{- if .Values.backend.ontoUrl -}}
{{ .Values.backend.ontoUrl }}
{{- else if .Values.ontoIntelli.enabled -}}
http://onto-intelli:{{ .Values.ontoIntelli.port }}
{{- end -}}
{{- end }}

{{/*
Where browsers reach onto-intelli's UI, which the workspace map embeds (NEXC_ONTO_PUBLIC_URL, and
the console's frame-src): backend.ontoPublicUrl, else ontoIntelli.ingress.host (https when the
ingress has TLS for it), else backend.ontoUrl. The release's own Service is never one: a browser
cannot reach it, so the chart refuses to render without one of the two.
*/}}
{{- define "nexc.ontoPublicUrl" -}}
{{- $o := .Values.ontoIntelli -}}
{{- if .Values.backend.ontoPublicUrl -}}
{{ .Values.backend.ontoPublicUrl }}
{{- else if .Values.backend.ontoUrl -}}
{{ .Values.backend.ontoUrl }}
{{- else if and $o.enabled $o.ingress.host -}}
{{- if not .Values.ingress.enabled -}}
{{- fail "ontoIntelli.ingress.host is routed by the chart's ingress: set ingress.enabled, or backend.ontoPublicUrl to where browsers reach onto-intelli" -}}
{{- end -}}
http{{ if include "nexc.tlsHost" (dict "host" $o.ingress.host "root" .) }}s{{ end }}://{{ $o.ingress.host }}
{{- else if $o.enabled -}}
{{- fail "ontoIntelli.enabled needs ontoIntelli.ingress.host or backend.ontoPublicUrl: where browsers reach onto-intelli's UI, which the workspace map embeds (the Service onto-intelli is not reachable from a browser)" -}}
{{- end -}}
{{- end }}

{{/* What runtime-egress lets through (runtime.egress.allow; guarded for --reuse-values from an
older release, which has no runtime.egress). */}}
{{- define "nexc.egressAllow" -}}
{{ dig "egress" "allow" "api.anthropic.com:443" .Values.runtime }}
{{- end }}

{{/*
The most backend pods that run at once: the autoscaler's maximum (or replicas.backend), plus the
one a rollout adds (maxSurge 1).
*/}}
{{- define "nexc.backendPods" -}}
{{- $b := .Values.autoscaling.backend -}}
{{- add1 (ternary $b.maxReplicas .Values.replicas.backend $b.enabled) -}}
{{- end }}

{{/*
The connections postgres.maxConnections leaves the backend: less postgres.reservedConnections and
onto-intelli's pools, replicas x maxConnections (its rollouts add no pod: maxSurge 0), guarded for
--reuse-values from a release without them.
*/}}
{{- define "nexc.dbBudget" -}}
{{- $pg := .Values.postgres -}}
{{- $onto := 0 -}}
{{- if .Values.ontoIntelli.enabled -}}
{{- $onto = mul .Values.ontoIntelli.replicas .Values.ontoIntelli.maxConnections -}}
{{- end -}}
{{- sub (sub (dig "maxConnections" 100 $pg) (dig "reservedConnections" 10 $pg)) $onto -}}
{{- end }}

{{/*
The backend's PostgreSQL pool per pod (NEXC_DB_MAX_CONNECTIONS): config.NEXC_DB_MAX_CONNECTIONS when
set, else each pod's share of the budget, at most 20 (the backend's own default). The realtime
listener holds one connection of it for good, so a share under 4 (one set under 2) is refused, and,
with the chart's PostgreSQL, so is a pool set that does not fit.
*/}}
{{- define "nexc.dbPool" -}}
{{- $pods := int (include "nexc.backendPods" .) -}}
{{- $budget := int (include "nexc.dbBudget" .) -}}
{{- $set := toString (.Values.config.NEXC_DB_MAX_CONNECTIONS | default "") -}}
{{- $pool := 0 -}}
{{- $least := 2 -}}
{{- if $set -}}
{{- $pool = int $set -}}
{{- else -}}
{{- $pool = int (min 20 (div (max $budget 0) $pods)) -}}
{{- $least = 4 -}}
{{- end -}}
{{- if or (lt $pool $least) (and .Values.postgres.enabled (gt (mul $pool $pods) $budget)) -}}
{{- fail (printf "PostgreSQL connections: %d backend pods at most (replicas.backend or autoscaling.backend.maxReplicas, plus one during a rollout) with a pool of %d each do not fit in the %d that postgres.maxConnections leaves after postgres.reservedConnections and onto-intelli's pools: raise postgres.maxConnections, or lower config.NEXC_DB_MAX_CONNECTIONS, the replicas or ontoIntelli.maxConnections" $pods $pool $budget) -}}
{{- end -}}
{{- $pool -}}
{{- end }}

{{/* The backend's environment, the same for `serve` and for the `migrate` init container. */}}
{{- define "nexc.backendEnv" -}}
envFrom:
  - configMapRef: { name: nexc-config }
  - secretRef: { name: {{ include "nexc.secretName" . }} }
{{- if .Values.oidc.enabled }}
env:
  # Single sign-on's client secret, from a Secret of its own.
  - name: NEXC_OIDC_CLIENT_SECRET
    valueFrom:
      secretKeyRef:
        name: {{ .Values.oidc.existingSecret | quote }}
        key: {{ .Values.oidc.existingSecretKey | quote }}
{{- end }}
{{- end }}
