## MODIFIED Requirements

### Requirement: Plain-HTTP forges only when allowed
With the instance setting `network.allowPlainHttp` on, the control plane SHALL build Git routes for `http://` repositories and the gateway SHALL connect to such upstreams without TLS. With the setting off, an `http://` repository SHALL get no Git route, and the gateway SHALL answer a route to an `http://` upstream with `502` without connecting. The sandbox SHALL use HTTPS to the gateway in both cases. The same setting governs forge plugin API URLs and model provider upstreams.

#### Scenario: Forge over plain HTTP in a throwaway cluster
- **WHEN** `network.allowPlainHttp` is on and the runtime of `DF-7` pushes its task branch while the project repository is `http://forgejo-http.forgejo.svc:3000/factory/demo`
- **THEN** the push goes over HTTPS to the gateway, and the gateway forwards it to Forgejo over plain HTTP

#### Scenario: Plain HTTP not allowed
- **WHEN** `network.allowPlainHttp` is off and a project's repository is `http://forgejo-http.forgejo.svc:3000/factory/demo`
- **THEN** the run gets no Git route and the gateway answers its clone with `403` (`df: no repository for this task`)
