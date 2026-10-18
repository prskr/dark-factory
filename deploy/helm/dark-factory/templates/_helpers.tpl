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
Certificate Issuer reference
*/}}
{{- define "dark-factory.issuerRef" -}}
{{- if .Values.tls.ca.create }}
name: {{ include "dark-factory.fullname" . }}-ca-issuer
kind: Issuer
group: cert-manager.io
{{- else }}
name: {{ .Values.tls.ca.existingIssuer.name }}
kind: {{ .Values.tls.ca.existingIssuer.kind | default "Issuer" }}
group: cert-manager.io
{{- end }}
{{- end }}
