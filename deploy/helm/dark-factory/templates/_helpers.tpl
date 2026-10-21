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
  runtimeImage: {{ .Values.sandboxNamespace.runtimeImage | quote }}
  runtimeDelivery: {{ .Values.sandboxNamespace.runtimeDelivery | quote }}
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
