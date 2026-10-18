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
