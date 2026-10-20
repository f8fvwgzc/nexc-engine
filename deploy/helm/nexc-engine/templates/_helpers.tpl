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

{{/* Whether `host` is served over TLS by the chart's ingress: `nexc.tlsHost (dict "host" … "root" $)`. */}}
{{- define "nexc.tlsHost" -}}
{{- $tls := false }}
{{- range .root.Values.ingress.tls }}
{{- if has $.host (.hosts | default list) }}{{ $tls = true }}{{ end }}
{{- end }}
{{- if $tls }}true{{ end }}
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
