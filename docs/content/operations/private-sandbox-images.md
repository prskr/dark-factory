---
title: Private sandbox images
weight: 5
---

Every sandbox pod pulls three images: the project image, the runtime image and the harness image.
When one of them lives in a private registry, give the pods an image pull secret.

## Set up a pull secret

1. Create a `kubernetes.io/dockerconfigjson` Secret in the **sandbox namespace**
   (`sandboxNamespace.name`, default `dark-factory-sandboxes`). Use a registry credential that can
   only pull.

   The commands read the token from the terminal, so it ends up neither in the shell history nor
   in the arguments of a process:

   ```sh
   read -rs TOKEN   # paste the pull-only token, then press Enter
   printf '{"auths":{"registry.example.org":{"auth":"%s"}}}' \
     "$(printf '%s:%s' '<user>' "$TOKEN" | base64 | tr -d '\n')" |
     kubectl create secret generic corp-mirror \
       --namespace dark-factory-sandboxes \
       --type=kubernetes.io/dockerconfigjson \
       --from-file=.dockerconfigjson=/dev/stdin
   unset TOKEN
   ```

2. Reference it in the Helm values and upgrade the release:

   ```yaml
   sandbox:
     imagePullSecrets:
       - name: corp-mirror
   ```

New runs use the secret. Pods that already exist keep the pod spec they were created with.

## What it covers

The secrets are listed on every sandbox pod, in the configured order. The kubelet uses the entry
that matches an image's registry, so one list serves the project image, the runtime image and the
harness image, also when they live in different registries.

- The secrets are never mounted into a container. The sandbox has no Kubernetes API token.
- The control plane gets no permission on Secrets in the sandbox namespace and does not check that
  the Secrets exist.
- `image.pullSecrets` is a separate setting for the control plane, gateway and migrate pods (instance
  namespace). It does not apply to sandboxes.

## Reference

| Setting | Required | Default | Effect |
| --- | --- | --- | --- |
| Helm value `sandbox.imagePullSecrets` | no | `[]` | List of `{ name: <Secret> }`. The chart renders the names as `sandbox.imagePullSecrets` and creates no Secret. An entry without a `name` fails the schema validation. |
| Instance key `sandbox.imagePullSecrets` | no | empty | List of Secret names in the sandbox namespace, applied to every sandbox pod. Each name must be a valid Kubernetes name (at most 253 characters: lowercase letters, digits, `-` and `.`, starting and ending with a letter or digit); otherwise the control plane refuses to start and names the option and the entry. |

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| The run ends `sandbox_lost` with `ImagePullBackOff` or `ErrImagePull` in its detail | The Secret does not exist in the sandbox namespace (check the name for typos: a name that is valid but wrong is not detected), holds credentials for another registry, or the credential has no pull access to the image. |
| `kubectl describe pod <run pod> -n <sandbox namespace>` shows `pull access denied` or `unauthorized` | The Secret is found but the credential is rejected: renew it. |
| The control plane exits at startup naming `sandbox.imagePullSecrets` | An entry is not a valid Secret name. |

See also [Troubleshooting](troubleshooting.md).
