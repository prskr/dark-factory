{{/*
Expand the name of the chart.
*/}}
{{- define "dark-factory.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "dark-factory.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "dark-factory.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "dark-factory.labels" -}}
helm.sh/chart: {{ include "dark-factory.chart" . }}
app.kubernetes.io/name: {{ include "dark-factory.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: dark-factory
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}

{{/*
Control plane selector labels
*/}}
{{- define "dark-factory.controlPlane.selectorLabels" -}}
app.kubernetes.io/name: {{ include "dark-factory.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: control-plane
{{- end }}

{{/*
Gateway selector labels
*/}}
{{- define "dark-factory.gateway.selectorLabels" -}}
app.kubernetes.io/name: {{ include "dark-factory.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: gateway
{{- end }}

{{/*
Control plane image reference (supports digest or tag)
*/}}
{{- define "dark-factory.controlPlane.image" -}}
{{- if .Values.image.controlPlane.digest }}
{{- printf "%s@%s" .Values.image.controlPlane.repository .Values.image.controlPlane.digest }}
{{- else }}
{{- printf "%s:%s" .Values.image.controlPlane.repository (.Values.image.controlPlane.tag | default .Chart.AppVersion) }}
{{- end }}
{{- end }}

{{/*
Runtime image reference (supports digest or tag)
*/}}
{{- define "dark-factory.runtime.image" -}}
{{- if .Values.image.runtime.digest }}
{{- printf "%s@%s" .Values.image.runtime.repository .Values.image.runtime.digest }}
{{- else }}
{{- printf "%s:%s" .Values.image.runtime.repository (.Values.image.runtime.tag | default .Chart.AppVersion) }}
{{- end }}
{{- end }}

{{/*
Claude Code harness image reference (supports digest or tag)
*/}}
{{- define "dark-factory.harness.claudeCode.image" -}}
{{- $img := .Values.image.harness.claudeCode -}}
{{- if $img.digest }}
{{- printf "%s@%s" $img.repository $img.digest }}
{{- else }}
{{- printf "%s:%s" $img.repository ($img.tag | default .Chart.AppVersion) }}
{{- end }}
{{- end }}

{{/*
Test project image reference (supports digest or tag)
*/}}
{{- define "dark-factory.testProject.image" -}}
{{- if .Values.image.testProject.digest }}
{{- printf "%s@%s" .Values.image.testProject.repository .Values.image.testProject.digest }}
{{- else }}
{{- printf "%s:%s" .Values.image.testProject.repository (.Values.image.testProject.tag | default .Chart.AppVersion) }}
{{- end }}
{{- end }}

{{/*
Control plane ServiceAccount name
*/}}
{{- define "dark-factory.controlPlane.serviceAccountName" -}}
{{- default (printf "%s-control-plane" (include "dark-factory.fullname" .)) .Values.controlPlane.serviceAccountName }}
{{- end }}

{{/*
Gateway ServiceAccount name
*/}}
{{- define "dark-factory.gateway.serviceAccountName" -}}
{{- default (printf "%s-gateway" (include "dark-factory.fullname" .)) .Values.gateway.serviceAccountName }}
{{- end }}

{{/*
Cluster-scoped name base: release namespace and fullname, so releases with the same name in
different namespaces do not collide on cluster-scoped objects or in the cert-manager namespace.
*/}}
{{- define "dark-factory.ca.base" -}}
{{- printf "%s-%s" .Release.Namespace (include "dark-factory.fullname" .) | trunc 40 | trimSuffix "-" }}
{{- end }}

{{/*
Name of the issuer that signs all certificates (control plane, gateway, sandboxes)
*/}}
{{- define "dark-factory.issuer.name" -}}
{{- if .Values.tls.ca.create }}
{{- printf "%s-ca-issuer" (include "dark-factory.ca.base" .) }}
{{- else }}
{{- .Values.tls.ca.existingIssuer.name }}
{{- end }}
{{- end }}

{{/*
Kind of the issuer that signs all certificates. The chart-created CA is always a ClusterIssuer:
sandbox certificates are requested from the sandbox namespace and can only use a ClusterIssuer.
*/}}
{{- define "dark-factory.issuer.kind" -}}
{{- if .Values.tls.ca.create }}ClusterIssuer{{- else }}{{ .Values.tls.ca.existingIssuer.kind | default "ClusterIssuer" }}{{- end }}
{{- end }}

{{/*
Certificate Issuer reference
*/}}
{{- define "dark-factory.issuerRef" -}}
name: {{ include "dark-factory.issuer.name" . }}
kind: {{ include "dark-factory.issuer.kind" . }}
group: cert-manager.io
{{- end }}

{{/*
Kind of the issuer for sandbox client certificates. Sandbox certificates are requested from the
sandbox namespace, where only a ClusterIssuer can be referenced, so anything else is refused.
*/}}
{{- define "dark-factory.sandbox.issuerKind" -}}
{{- $kind := .Values.sandboxNamespace.issuerKind | default (include "dark-factory.issuer.kind" .) }}
{{- if ne $kind "ClusterIssuer" }}
{{- fail (printf "sandbox certificates need a ClusterIssuer but the issuer kind is %q: set sandboxNamespace.issuerName and sandboxNamespace.issuerKind=ClusterIssuer (or use tls.ca.existingIssuer.kind=ClusterIssuer)" $kind) }}
{{- end }}
{{- $kind }}
{{- end }}

{{/*
The `sandbox:` section of the instance configuration (shared by the config and migrate ConfigMaps).
*/}}
{{- define "dark-factory.sandboxConfig" -}}
sandbox:
  logLevel: {{ .Values.sandbox.logLevel | quote }}
  namespace: {{ .Values.sandboxNamespace.name | quote }}
  instanceNamespace: {{ .Release.Namespace | quote }}
  serviceAccountName: {{ .Values.sandboxNamespace.serviceAccountName | quote }}
  {{- if hasKey .Values.sandboxNamespace "storageClass" }}
  storageClass: {{ .Values.sandboxNamespace.storageClass | quote }}
  {{- end }}
  pvcSize: {{ .Values.sandboxNamespace.pvcSize | quote }}
  {{- if hasKey .Values.sandboxNamespace "runtimeClassName" }}
  runtimeClassName: {{ .Values.sandboxNamespace.runtimeClassName | quote }}
  {{- end }}
  issuerName: {{ .Values.sandboxNamespace.issuerName | default (include "dark-factory.issuer.name" .) | quote }}
  issuerKind: {{ include "dark-factory.sandbox.issuerKind" . | quote }}
  issuerGroup: {{ .Values.sandboxNamespace.issuerGroup | quote }}
  certificateDuration: {{ .Values.sandboxNamespace.certificateDuration | quote }}
  certificateRenewBefore: {{ .Values.sandboxNamespace.certificateRenewBefore | quote }}
  runtimeImage: {{ include "dark-factory.runtime.image" . | quote }}
  clusterDomain: {{ .Values.sandboxNamespace.clusterDomain | quote }}
  controlPlaneServiceName: {{ printf "%s-control-plane" (include "dark-factory.fullname" .) | quote }}
  gatewayServiceName: {{ printf "%s-gateway" (include "dark-factory.fullname" .) | quote }}
  controlPlaneTlsServerName: {{ printf "%s-control-plane" (include "dark-factory.fullname" .) | quote }}
  {{- if .Values.sandboxNamespace.controlPlaneIp }}
  controlPlaneIp: {{ .Values.sandboxNamespace.controlPlaneIp | quote }}
  {{- end }}
  {{- if .Values.sandboxNamespace.gatewayIp }}
  gatewayIp: {{ .Values.sandboxNamespace.gatewayIp | quote }}
  {{- end }}
{{- end }}

{{/*
Whether the control plane and gateway serve Prometheus metrics
*/}}
{{- define "dark-factory.telemetry.prometheus" -}}
{{- if has "prometheus" .Values.telemetry.metrics.exporters }}true{{- end }}
{{- end }}

{{/*
OTEL_* environment of the control plane and gateway containers.
Arguments: dict "ctx" <root context> "component" <service name suffix>
*/}}
{{- define "dark-factory.telemetry.env" -}}
{{- $t := .ctx.Values.telemetry -}}
- name: OTEL_SERVICE_NAME
  value: {{ printf "dark-factory-%s" .component | quote }}
- name: OTEL_METRICS_EXPORTER
  value: {{ if $t.metrics.exporters }}{{ join "," $t.metrics.exporters | quote }}{{ else }}"none"{{ end }}
{{- if has "prometheus" $t.metrics.exporters }}
- name: OTEL_EXPORTER_PROMETHEUS_PORT
  value: {{ $t.metrics.port | quote }}
{{- end }}
- name: OTEL_TRACES_EXPORTER
  value: {{ if $t.traces.enabled }}"otlp"{{ else }}"none"{{ end }}
{{- if $t.traces.enabled }}
- name: OTEL_TRACES_SAMPLER
  value: {{ $t.traces.sampler | quote }}
{{- if $t.traces.samplerArg }}
- name: OTEL_TRACES_SAMPLER_ARG
  value: {{ $t.traces.samplerArg | quote }}
{{- end }}
{{- end }}
{{- if $t.otlp.endpoint }}
- name: OTEL_EXPORTER_OTLP_ENDPOINT
  value: {{ $t.otlp.endpoint | quote }}
- name: OTEL_EXPORTER_OTLP_PROTOCOL
  value: {{ $t.otlp.protocol | quote }}
{{- if $t.otlp.insecure }}
- name: OTEL_EXPORTER_OTLP_INSECURE
  value: "true"
{{- end }}
{{- if $t.otlp.headersSecret }}
- name: OTEL_EXPORTER_OTLP_HEADERS
  valueFrom:
    secretKeyRef:
      name: {{ $t.otlp.headersSecret | quote }}
      key: headers
{{- end }}
{{- end }}
{{- end }}

{{/*
NetworkPolicy ingress rule that lets the metrics scraper reach the metrics port; renders nothing
without a scraper namespace selector or without Prometheus metrics. Only a
`kubernetes.io/metadata.name` selector that names the sandbox namespace is rejected; other labels
the sandbox namespace shares with the selector are not checked (its default-deny egress blocks
sandboxes anyway).
*/}}
{{- define "dark-factory.netpol.scraper" -}}
{{- $scraper := .Values.networkPolicy.metricsScraper -}}
{{- if eq (index $scraper.namespaceSelector "kubernetes.io/metadata.name" | default "") .Values.sandboxNamespace.name -}}
{{- fail "networkPolicy.metricsScraper.namespaceSelector must not select the sandbox namespace: sandboxes must not reach the metrics port" -}}
{{- end -}}
{{- if and $scraper.namespaceSelector (include "dark-factory.telemetry.prometheus" .) -}}
- from:
    - namespaceSelector:
        matchLabels:
          {{- toYaml $scraper.namespaceSelector | nindent 10 }}
      {{- if $scraper.podSelector }}
      podSelector:
        matchLabels:
          {{- toYaml $scraper.podSelector | nindent 10 }}
      {{- end }}
  ports:
    - protocol: TCP
      port: {{ .Values.telemetry.metrics.port }}
{{- end }}
{{- end }}
