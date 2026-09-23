# Authentication and authorization boundary

Crono currently uses a development identity that deliberately accepts every
operation. It is not an absence of authorization code: each HTTP request is
assigned the server-owned principal `development/local`, and every application
use case asks an injected `Authorizer` for a typed decision before it accesses
persistence. Request bodies and headers cannot choose a principal, role, or
permission.

The current `PermitAllAuthorizer` grants those decisions and returns visibility
over all Namespaces. This keeps local domain and NATS testing frictionless while
exercising the boundary that a production policy will replace. Startup emits a
warning so the active trust model is visible. This mode is suitable only for a
local development server behind a trusted network boundary.

## Flow overview

```text
HTTP request
  -> identity middleware creates RequestContext
  -> application parses canonical resource identity
  -> Authorizer checks Capability + ResourceScope
  -> Authorizer supplies Namespace visibility for reads
  -> PostgreSQL applies visibility before pagination/counting
  -> application returns domain records to the HTTP mapper
```

The `RequestContext` holds an opaque principal and a per-request correlation
UUID. `Capability` is the stable permission vocabulary: Namespace create/read,
Job create/read/execute, Target create/read/use, and Run create/read.
Schedule create/read/update and global Worker read complete the current
vocabulary; worker presence is operational control-plane metadata rather than
Namespace-owned data.
`ResourceScope` identifies the control plane, Namespace, qualified Job,
qualified Target, or Run involved in one decision. Run creation checks all
three relevant permissions: creating the Run, executing the selected Job, and
using the selected Target.

List and count operations use `VisibilityScope`, which is either all
Namespaces, a server-derived set of Namespace IDs, or none. The PostgreSQL
adapter applies that scope inside its query, before pagination and aggregation.
Reading an individual Run also applies the scope and returns not found when the
Run is outside it, preventing existence disclosure. Authorization helpers are
side-effect free; the authoritative database mutation occurs only after every
required decision succeeds.

## Replacing development access

A credential verifier should replace only the development identity middleware.
It validates a session, token, or mTLS identity and constructs the same
`RequestContext`; it must never copy roles or permissions directly from
unverified client input. A production `Authorizer` then replaces
`PermitAllAuthorizer` and evaluates the existing capabilities and scopes using
verified claims plus authoritative server-side policy data.

The transport, application use cases, domain model, PostgreSQL schema, and NATS
dispatch format do not need an authentication refactor for that replacement.
If production policy becomes an external dependency, failures map to
`AuthorizationError::Unavailable` and deny access. Unauthenticated and
forbidden decisions already map to distinct HTTP responses, while internal
policy detail is not exposed to clients.

Authentication ownership is intentionally absent from the current database
schema. Namespaces, Jobs, Targets, Runs, and the outbox model domain state; they
do not contain provisional users, roles, password hashes, tokens, or provider
identifiers. Identity and role storage should be added only with the selected
authentication protocol and lifecycle requirements.
