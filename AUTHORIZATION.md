# Authentication and authorization boundary

Authentication establishes who supplied a valid credential. Authorization is
Crono-owned and determines whether that verified principal may perform a typed
`Capability` on a `ResourceScope`, with `VisibilityScope` restricting reads.
These are independently injected dependencies. HTTP handlers, application use
cases, repositories, domain records, PostgreSQL, and NATS never parse tokens,
JWTs, OAuth/OIDC claims, or provider-specific roles.

## Current request flow

```text
HTTP request
     |
     v
server-issued request ID + trace span
     |
     v
HTTP Authorization: Bearer <opaque token> extraction
     |
     v
DevelopmentAuthProvider (configured static secret)
     |
     v
development/local Principal
     |
     v
RequestContext
     |
     v
PermitAllAuthorizer
     |
     v
Capability + ResourceScope + VisibilityScope
     |
     v
Crono application/domain operation
```

`AuthProvider::authenticate(&RequestCredentials)` is asynchronous and independent
of HTTP. The adapter accepts exactly one Authorization header with a
case-insensitive Bearer scheme, one or more ASCII spaces, and a case-sensitive
opaque token. The token alphabet follows [RFC 6750](https://www.rfc-editor.org/rfc/rfc6750.html#section-2.1),
with an 8 KiB bound; only trailing `=` padding is allowed. Duplicate/combined
headers, unsupported schemes, malformed values, and missing credentials are
rejected before handlers parse input. Query parameters, request bodies, cookies,
and principal/role/capability headers cannot supply trusted identity or authority.
Only the provider's successful result can create the HTTP `RequestContext`.

The development verifier accepts exactly one configured secret and produces
`development/local` with `PrincipalKind::Development`. Token bytes never become
a principal identifier. `subtle` compares content in constant time for equal
lengths; token length is not concealed. Credential/config/provider Debug output
is redacted, and errors and HTTP traces do not contain credentials. Authentication
failure never invokes the application or selects a permissive fallback.

`PermitAllAuthorizer` currently grants all defined capabilities and Namespace
visibility after authentication. The token check does not bypass it: replacing
the verifier does not change authorization, and replacing the policy does not
change credential verification. Startup warns that this is temporary full-access
development policy. Use it only in a controlled development/test environment.

GET/HEAD on `/live`, `/ready`, `/health`, and `/metrics` remain public for probes
and monitoring. They do not execute resource use cases. All other requests,
including API fallbacks, require authentication. Keep operational endpoints behind
an appropriate deployment network boundary. Public HTTP requires TLS termination;
cleartext loopback is only a local development convenience.

## Configuration and development clients

`--auth-mode` / `CRONO_AUTH_MODE` selects the provider and defaults to `development`.
`CRONO_AUTH_DEVELOPMENT_TOKEN` is mandatory, with no default or command-line token
argument. Set it to a randomly generated Bearer value of 32–8192 bytes before
starting the server or `just dev-start`, for example:

```sh
export CRONO_AUTH_MODE=development
export CRONO_AUTH_DEVELOPMENT_TOKEN="$(openssl rand -hex 32)"
just dev-start
```

Keep the value private; do not commit it or enable shell tracing while handling
it. Short, malformed, missing, or non-Unicode configuration fails startup before
PostgreSQL, NATS, or the listener is opened. Selecting `oidc` fails explicitly
because that provider is not implemented, even if a development token is present.
There are no unused issuer/audience/discovery placeholders in configuration.

The web client reads the opaque credential centrally from browser **session
storage**, scoped to the frontend origin and tab. Set it in that tab's DevTools
console using the actual configured secret, then reload:

```js
sessionStorage.setItem('crono.access_token', '<configured development token>');
location.reload();
```

Clear it with `sessionStorage.removeItem('crono.access_token')`. It is attached to
GET/POST/PUT/PATCH/DELETE requests as an Authorization header, never built into the
WASM bundle or sent in URLs. Missing credentials receive the server's 401 envelope.
This temporary setup adds no login screen, cookie session, token issuance, or
refresh flow. Browser storage is not an identity authority: the server still
verifies every supplied credential. The CLI currently has no HTTP transport;
future client requests must supply Bearer credentials through that client boundary.

## Identity and authorization invariants

`Principal` carries an opaque subject, optional verified issuer, and caller kind.
An external identity is the **pair `(issuer, subject)`**; identical subjects from
different issuers differ. Email and human profile fields are not identity keys or
required fields. Human, service, system, and development principals share this
provider-neutral representation. Only trusted provider/server code constructs it
after verification; neither Principal nor RequestContext is deserialized from a
client request. Future authority metadata must likewise be verified and neutral.

The server issues each correlation UUID and returns it in `x-request-id`. A
client-supplied request ID is never adopted as that UUID. RequestContext pairs it
with the verified principal; it carries no JWT or OIDC claim object.

Application use cases still ask the injected `Authorizer` for capabilities and
resource scopes before persistence. Run creation checks Run creation, execution
of the selected Job, and use of the selected Target. Global Worker and Monitor
reads remain separate capabilities. Policy decisions are side-effect free and
must use verified identity plus authoritative server policy, never client roles.

Visibility is all Namespaces, a server-derived set of Namespace IDs, or none.
The PostgreSQL adapter applies that scope before pagination, counts, and
aggregation. An individual Run outside visibility remains not found, preserving
resource-hiding behavior. Authentication does not move any of these decisions
into middleware or query Jobs, Targets, Runs, or other Crono resources.

Missing, malformed, unsupported, or invalid credentials return 401 with the safe
`unauthenticated` envelope and `WWW-Authenticate: Bearer realm="crono"`.
Verifier outages return a generic 503 without running authorization or handlers.
An authenticated denial remains 403, or the existing 404 visibility semantics.
Authorization outages remain 503. No failure establishes a development identity
or falls back to PermitAll.

## Future external authentication

```text
Bearer access token
     |
     v
OIDC/OAuth AuthProvider
(Permesi / Auth0 / Keycloak / Zitadel / another IAM)
     |
     v
verified Principal
     |
     v
RequestContext
     |
     v
Production Authorizer
     |
     v
Capability + ResourceScope + VisibilityScope
     |
     v
Crono
```

Replace `DevelopmentAuthProvider` with an `OidcAuthProvider` (or an opaque-token
introspection/workload credential verifier) at startup/router injection. Its
configuration and implementation own trusted issuer, audience, signature,
expiry/not-before, subject, and any required scope/claim validation. The stable
result is Principal, so Jobs, Targets, Runs, and other application logic remain
unchanged. Additional credential forms such as mTLS can extend the transport
adapter and RequestCredentials without making JWT a domain concept.

Independently replace `PermitAllAuthorizer` with a production Authorizer that
interprets verified authority and authoritative Crono policy through the existing
capability/resource/visibility contract. No complex RBAC or provisional IAM is
introduced here. A fake external provider integration test exercises the same
Namespace handler and injected policy using an issuer-scoped service principal.

Crono's intended role is an OAuth2 resource server receiving access tokens. IAM
providers own passwords, signup, resets, MFA/passkeys, login screens, grants,
authorization-code exchange, and refresh-token issuance. None is implemented by
this refactor. Crono's domain schema stores workload state, not users, passwords,
provider claims, or tokens.
