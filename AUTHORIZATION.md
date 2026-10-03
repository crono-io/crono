# Authentication and authorization boundary

Authentication establishes who supplied a valid credential. Authorization is
Crono-owned and determines whether the verified caller and its scoped grants may perform a typed
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
AuthenticatedCaller (development/local + explicit grants)
     |
     v
RequestContext
     |
     v
GrantAuthorizer
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
Only the provider's successful `AuthenticatedCaller` result can create the HTTP
`RequestContext`, carrying verified identity and normalized authority.

The development verifier accepts exactly one configured secret and produces
`development/local` with `PrincipalKind::Development` and explicit global and
all-Namespace assignments for every registered permission. Token bytes never become
a principal identifier. `subtle` compares content in constant time for equal
lengths; token length is not concealed. Credential/config/provider Debug output
is redacted, and errors and HTTP traces do not contain credentials. Authentication
failure never invokes the application or selects a permissive fallback.

`GrantAuthorizer` is the default runtime policy. It denies unless the caller has
the requested permission in the resource's actual scope. The development verifier
issues full grants after a successful token match, so existing development clients
retain full access through the same evaluator as restricted callers. Startup warns
about these full-access development grants. `PermitAllAuthorizer` remains an
explicit test fixture and is never the runtime default or a failure fallback.

GET/HEAD on `/live`, `/ready`, `/health`, and `/metrics` remain public for probes
and monitoring. They do not execute resource use cases. All other requests,
including API fallbacks, require authentication. Keep operational endpoints behind
an appropriate deployment network boundary. Public deployments require TLS termination;
the cleartext development stack is intended for a trusted testing network.

## Configuration and development clients

`--auth-mode` / `CRONO_AUTH_MODE` selects the provider and defaults to `development`.
`CRONO_AUTH_DEVELOPMENT_TOKEN` is mandatory, with no default or command-line token
argument in the server. For the local stack, simply run:

```sh
just dev-start
```

The web listener binds `0.0.0.0` by default. Open `http://127.0.0.1:3000` locally
or `http://<server-ip>:3000` from a laptop on the testing network. The launcher
generates 256 random bits using OpenSSL
when no token is configured, saves the hex value in `target/dev-auth/token`, and
reuses it across restarts. The ignored directory has mode 0700; the token and
generated Trunk configuration have mode 0600. An explicitly supplied environment
token takes priority without replacing the persisted default. An existing invalid
token fails startup; it is never silently regenerated. Invalid authentication
configuration is checked before stopping a working stack.

For local and remote development browsers, Trunk adds the configured Bearer header to
proxied `/api` requests. The API still uses `DevelopmentAuthProvider`, RequestContext,
and the independent Authorizer; direct API calls without a token still return 401.
The credential stays outside browser assets and URLs. Proxy header logging is
disabled even if the parent shell requests trace logging, and the proxy neither
uses system HTTP proxies nor follows redirects. The generated configuration follows
the selected API port. It is private runtime configuration, not a deployment artifact.

Every client that can reach the development web port receives the configured
development identity and its current full-access authorization. Keep this proxy
on a trusted testing network. Use `just dev-start 127.0.0.1` when only local
access is needed. Binding changes where the web client can be reached; it does
not change the API authentication provider or its independent Authorizer.
Production web hosting never supplies a development credential.

When launching the server separately, set a randomly generated Bearer value of
32–8192 bytes yourself, for example:

```sh
export CRONO_AUTH_MODE=development
export CRONO_AUTH_DEVELOPMENT_TOKEN="$(openssl rand -hex 32)"
just server
```

Keep the value private; do not commit it or enable shell tracing while handling
it. Short, malformed, missing, or non-Unicode configuration fails startup before
PostgreSQL, NATS, or the listener is opened. Selecting `oidc` fails explicitly
because that provider is not implemented, even if a development token is present.
There are no unused issuer/audience/discovery placeholders in configuration.

Outside the automatic local proxy, the web client reads the opaque credential
centrally from browser **session storage**, scoped to the frontend origin and tab.
Set it in that tab's DevTools
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
after verification. `AuthenticatedCaller` pairs this identity with an immutable
`GrantSet`. None of these types is deserialized from HTTP input. Converting an
identity alone into a caller yields empty authority, including in test adapters.

The server issues each correlation UUID and returns it in `x-request-id`. A
client-supplied request ID is never adopted as that UUID. RequestContext pairs it
with the verified caller; it carries no token, JWT, or OIDC claim object.

Application use cases still ask the injected `Authorizer` for capabilities and
resource scopes before persistence. Run creation checks Run creation, execution
of the selected Job, and use of the selected Target. Global Worker and Monitor
reads remain separate capabilities. Policy decisions are side-effect free and
must use verified grants and authoritative resource ownership, never client roles.
A narrow `ResourceNamespaceResolver` reads only Namespace IDs for UUID resources.
Jobs, Targets, Target Sets, Schedules, and Workflows use their own Namespace; Runs
resolve through their Job, and Workflow Runs use their stored Namespace. Full
resource payloads are not fetched to make this decision. Explicit all-Namespace
grants and direct Namespace references avoid this lookup.

Target Set replacement validates generic request fields, then checks update
authority before loading configuration. A caller without update grants receives
403 for both existing and nonexistent set IDs. Authorized replacements still
require Target read on each member and membership in the set's Namespace.

Visibility is all Namespaces, a server-derived set of Namespace IDs, or none.
Each read permission computes its own scope; unrelated assignments cannot widen
it. The PostgreSQL adapter applies that scope before pagination, counts, and
aggregation. `crono.overview.read` independently controls aggregate workload counts;
Namespace metadata reads never grant counts. Missing grants yield empty unscoped
Namespace/Run pages and zero overview counts. A scoped list still requires its
read permission on the requested Namespace. Runs, Workflow graphs, and Workflow
Runs outside visibility return not found,
including Attempt output and events. Other denied operations return forbidden.
Missing ownership metadata returns not found; resolver outages fail closed with
503 rather than treating missing policy data as permission. Authentication does
not move any of these decisions
into middleware or query Jobs, Targets, Runs, or other Crono resources.

Missing, malformed, unsupported, or invalid credentials return 401 with the safe
`unauthenticated` envelope and `WWW-Authenticate: Bearer realm="crono"`.
Verifier outages return a generic 503 without running authorization or handlers.
An authenticated denial remains 403, or the existing 404 visibility semantics.
Authorization outages remain 503. No failure establishes a development identity
or falls back to PermitAll.

## Canonical grants, version 1

The transport-independent [JSON Schema](docs/authorization/grants-v1.schema.json)
defines the authority interchange. Its document is at most 64 KiB of UTF-8 JSON,
with at most 256 raw assignments, including duplicates and empty assignments.
The parser rejects unknown fields at every level, duplicate fields, unknown
permissions, unsupported versions, invalid Namespace UUIDs, and incompatible
scope/permission combinations. UUIDs use the standard hyphenated representation;
uppercase hex is accepted and normalized. The typed constructor likewise validates
scope compatibility and assignment bounds. Encoding a typed set enforces the wire
size limit. Neither parsing nor encoding performs credential verification.

```json
{
  "version": 1,
  "grants": [
    {
      "scope": {
        "kind": "namespace",
        "namespace_id": "019a0000-0000-7000-8000-000000000001"
      },
      "permissions": [
        "crono.job.execute",
        "crono.target.use",
        "crono.run.create"
      ]
    },
    {
      "scope": {
        "kind": "namespace",
        "namespace_id": "019a0000-0000-7000-8000-000000000002"
      },
      "permissions": ["crono.job.read"]
    },
    {
      "scope": {"kind": "global"},
      "permissions": ["crono.queue.read"]
    }
  ]
}
```

This caller may execute in the first Namespace and read Job definitions in the
second. Read access in the second cannot grant execution there, and execute access
in the first cannot grant definition reads there. Permissions and Namespace IDs
are never flattened into a cross product. Duplicate permissions/assignments
combine only within the same exact scope; empty arrays grant nothing. There are
no permission wildcards, role inheritance, deny rules, or implicit permissions.

`global` accepts only global permissions and requires no Namespace ID.
`namespace` requires one authoritative Namespace UUID and only namespaced
permissions. `all_namespaces` requires no UUID, accepts only namespaced permissions,
and includes current and future Namespaces for exactly the listed permissions.
An assignment covers every resource of the listed kinds in its Namespace,
including resources created later. Version 1 has no per-Job or per-Target grants.
It never grants Queue, Worker, or Monitor access. Namespace creation is global
but does not assign authority over the new Namespace.

IAM may define a reader role from the desired read permissions, an executor role
from Job execute, Target/Target Set use, and Run create, or an operator role from
the required global permissions. Those names and role management remain entirely
in IAM. Assign the role at a Namespace, expand it into canonical assignments,
and apply any narrower token delegation limits before returning grants. Namespace
management, executable Job updates, and global Queue management are independent
powerful permissions; none is implied by a role name such as `admin`.

Identifiers and scope meanings below are frozen for version 1. Rust variant
renaming never changes a permission identifier. Unknown identifiers fail closed;
deploy receivers that understand an added identifier before issuers use it.
Incompatible meanings require a new contract version and explicit adapter rollout,
never silent remapping or fallback. The registry, schema enums, and identifier
fixture are tested together.

| Permission | Assignment scope | Requested resource kinds | Behavior |
| --- | --- | --- | --- |
| `crono.namespace.create` | global | ControlPlane | Create a Namespace; does not assign access to it. |
| `crono.namespace.read` | namespace / all_namespaces | Namespace | Read Namespace metadata; does not expose workload counts. |
| `crono.namespace.delete` | namespace / all_namespaces | Namespace | Delete an empty Namespace subject to bootstrap protections. |
| `crono.queue.create` | global | ControlPlane | Create a global worker Queue. |
| `crono.queue.read` | global | ControlPlane, Queue | Read global Queues, including selecting a Queue for a Job. |
| `crono.queue.update` | global | Queue | Edit or enable a global Queue. |
| `crono.queue.delete` | global | Queue | Delete an unused non-system Queue. |
| `crono.job.create` | namespace / all_namespaces | Namespace | Create executable Job definitions in a Namespace. |
| `crono.job.read` | namespace / all_namespaces | Namespace, Job | Read Job definitions and execution configuration. |
| `crono.job.update` | namespace / all_namespaces | Job | Replace executable configuration for future executions. |
| `crono.job.execute` | namespace / all_namespaces | Job | Execute a Job; Run creation and selected Target use are checked separately. |
| `crono.target.create` | namespace / all_namespaces | Namespace | Create a Target's arguments and inputs. |
| `crono.target.read` | namespace / all_namespaces | Namespace, Target | Read Target arguments and inputs. |
| `crono.target.update` | namespace / all_namespaces | Target | Replace a Target's arguments and inputs for future executions. |
| `crono.target.delete` | namespace / all_namespaces | Target | Delete an unused Target subject to starter protections. |
| `crono.target.use` | namespace / all_namespaces | Target | Use a Target's arguments and inputs for execution. |
| `crono.target_set.create` | namespace / all_namespaces | Namespace | Create a Target Set; member Target reads are checked separately. |
| `crono.target_set.read` | namespace / all_namespaces | Namespace, TargetSet | Read Target Set configuration and membership. |
| `crono.target_set.update` | namespace / all_namespaces | TargetSet | Replace Target Set membership and inputs, affecting future scheduled executions. |
| `crono.target_set.use` | namespace / all_namespaces | TargetSet | Use a Target Set; each selected Target requires its own use grant. |
| `crono.schedule.create` | namespace / all_namespaces | Namespace | Create recurring or deferred execution; underlying execution grants are required. |
| `crono.schedule.read` | namespace / all_namespaces | Namespace, Schedule | Read Schedule timing, references, and policy. |
| `crono.schedule.update` | namespace / all_namespaces | Schedule | Enable or disable a Schedule; enabling requires execution grants. |
| `crono.workflow.create` | namespace / all_namespaces | Namespace | Create a Workflow graph; referenced Job reads are checked separately. |
| `crono.workflow.read` | namespace / all_namespaces | Namespace, Workflow | Read Workflow graphs in visible Namespaces. |
| `crono.workflow.update` | namespace / all_namespaces | Workflow | Replace a Workflow graph at its expected revision. |
| `crono.workflow.delete` | namespace / all_namespaces | Workflow | Delete a Workflow subject to invocation references. |
| `crono.workflow.execute` | namespace / all_namespaces | Workflow | Launch a Workflow; all underlying execution grants are checked separately. |
| `crono.workflow_run.read` | namespace / all_namespaces | Workflow, WorkflowRun | Read invocation graphs and child Run identities; output needs Run read. |
| `crono.workflow_run.cancel` | namespace / all_namespaces | WorkflowRun | Cancel pending Workflow work; invocation read is also required, with no process-kill authority. |
| `crono.run.create` | namespace / all_namespaces | Namespace | Commit execution intent in a Namespace; Job and Target grants remain required. |
| `crono.run.read` | namespace / all_namespaces | Run | Read Run history, Attempt output, and lifecycle events. |
| `crono.worker.read` | global | ControlPlane | Read global worker presence and bounded diagnostics. |
| `crono.monitor.read` | global | ControlPlane | Read global operational and database monitoring. |
| `crono.overview.read` | namespace / all_namespaces | Namespace | Read aggregate workload counts for assigned Namespaces. |

## Workflow authorization

Workflow catalog operations and invocation reads/cancellation use typed Workflow
capabilities in the same Authorizer. Launch requires WorkflowExecute, every
referenced JobExecute, each pinned TargetUse, TargetSetUse where selected, and
Namespace RunCreate. The graph revision and exact authorized membership are
checked transactionally before immutable intent commits. Orchestration then
consumes that intent without granting new authority. WorkflowRead/WorkflowRunRead
visibility hides other Namespaces; child Attempt output independently requires
RunRead. See [WORKFLOWS.md](WORKFLOWS.md).

Cancellation requires both Workflow Run read visibility and cancel permission
before changing state or returning the invocation. Its response contains graph,
inputs, and child history, including for finished invocations; a cancel-only
caller receives 404 and commits no change.

Creating a Schedule, or enabling one, requires Schedule create or read/update
respectively, Job read/execute, Namespace Run create, and selected Target use.
Target Set selection also requires Target Set use and use of every current member.
Denied operations commit no Schedule change or execution intent. Disabling uses
Schedule read/update alone. Manual Run creation, Workflow launches, and their
idempotent replays check every selected execution permission. A historical rerun
repeats one retained Target snapshot and requires Run read, Job execute, Target
use, and Run create. Target Set use applies when selecting a set for a new batch.
Reading a graph never grants execution or child output access.

Schedules and Workflows commit server-owned execution intent. Background services
execute that intent without retaining tokens or calling IAM for each occurrence
or node. Later grant revocation affects new requests and replays; it does not
automatically disable an existing Schedule or cancel committed Workflow work.
Disable or cancel that work explicitly when needed. Changing executable Job or
Target inputs remains separately authorized and affects future executions.
Target Set updates likewise affect future scheduled membership and inputs:
Schedules resolve their selected set at each occurrence. The enabling caller's
Namespace Target use grant covers every member in that Namespace, including
future members. Workflow invocations pin membership for immutable history instead.

## Future external authentication

```text
Bearer access token
     |
     v
OIDC/OAuth AuthProvider
(Permesi / Auth0 / Keycloak / Zitadel / another IAM)
     |
     v
AuthenticatedCaller (Principal + GrantSet)
     |
     v
RequestContext
     |
     v
GrantAuthorizer
     |
     v
Capability + ResourceScope + VisibilityScope
     |
     v
Crono
```

Replace `DevelopmentAuthProvider` with a JWT verifier, opaque-token introspection
adapter, or workload credential verifier at startup/router injection. Its
configuration and implementation own trusted issuers, Crono audience, signatures
and key rotation, expiry/not-before, subjects, revocation/caching policy, and
delegation limits. After verification, it normalizes provider-native authority
into `GrantSet` and returns `AuthenticatedCaller`. Grant parsing alone authenticates
nothing. A present malformed/unsupported grant document rejects authentication
with the safe 401 envelope; an absent document establishes identity with no grants.
Adapters must not swallow validation errors, union unrelated resource audiences,
or expand delegated access beyond the verified token's limits. Verifier outages
return 503. Provider-native role names never reach application use cases.

IAM owns users, groups, roles, and assignments. Permesi, Keycloak, Auth0, and other
providers can map different verified claim layouts or introspection responses into
the same contract. Crono owns the permission vocabulary and checks each referenced
resource itself. No particular JWT claim name is mandated: providers may emit the
canonical JSON or adapters may translate verified native assignments through the
validated typed constructor. Crono never calls an IAM for every resource decision;
a future adapter decides whether credential verification requires introspection.

The production evaluator already exists and is provider-independent. Fake
provider integration tests exercise two authority layouts through the unchanged
HTTP handlers, production policy, and PostgreSQL membership resolver. External
OIDC/introspection, IAM role administration, login, and token refresh remain future
work. Selecting `oidc` still fails explicitly. Additional credential forms such as
mTLS can extend transport without making provider claims a domain concept.

Crono's intended role is an OAuth2 resource server receiving access tokens. IAM
providers own passwords, signup, resets, MFA/passkeys, login screens, grants,
authorization-code exchange, and refresh-token issuance. None is implemented by
this refactor. Crono's domain schema stores workload state, not users, passwords,
provider claims, or tokens.
