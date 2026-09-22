# Workload domain model

Crono organizes executor-agnostic workloads around a strict separation between
what should happen and where it should happen:

> Crono models WHAT to execute separately from WHERE to execute it. Jobs describe behavior;
> Targets describe execution destinations/resources. Executor-specific concepts such as Ansible
> inventory remain outside the core domain.

```text
Namespace
   |
   +-- Job -> JobVersion
   |
   +-- Target
   |
   +-- TargetSet

JobVersion + Target/TargetSet + Inputs
                    |
                    v
                   Run
                    |
               RunAttempt
                    |
                  Worker
                    |
                 Executor
```

`Job = what to do`, `Target = where or against what to do it`, and `Run = one
requested execution`. A Namespace organizes these resources but does not take
part in execution. A TargetSet is only a named, explicit set of Targets in the
same Namespace; it is not a placement rule, label selector, or worker group.
Whether a TargetSet request creates one Run or fans out to several Runs remains
undefined until Run semantics are designed.

## Identities and names

Namespace and resource names are lowercase path-safe segments. They begin and
end with an ASCII letter or digit and may contain lowercase ASCII letters,
digits, and hyphens. Examples include `mariadb`, `database-prod`, `backup`, and
`host-123`; empty names, path traversal, slashes, spaces, uppercase letters,
underscores, and leading or trailing hyphens are rejected by one domain
validator.

The domain uses separate `NamespaceId`, `JobId`, `TargetId`, and `TargetSetId`
types as stable internal identities. External interfaces may derive canonical
qualified names such as `mariadb/backup` and `mariadb/host-123` from the owning
Namespace and resource name. The qualified string is not stored as another
identity and cannot be derived across mismatched Namespace relationships.

## Concepts

| Concept | Meaning |
| --- | --- |
| Namespace | Logical organizational boundary containing Jobs, Targets, and Target Sets |
| Job | Stable identity for what Crono should execute, independent of any destination |
| JobVersion | Immutable execution definition belonging to a Job |
| Target | Stable identity for where or against what a Job executes |
| TargetSet | Named explicit selection of unique Targets from one Namespace |
| Run | One requested execution with inputs and pinned definitions |
| RunAttempt | One attempt to perform a Run, with its own worker assignment and outcome |
| Worker | Identified execution process serving authorized queues |
| Executor | Runtime adapter that interprets a pinned execution specification |

The first vertical slice persists organizational identities and relationships,
immutable no-op Job versions, and Runs through acknowledged dispatch. `Target`
deliberately has no generic JSON configuration field. When an executor
configuration contract exists, the appropriate execution layer will interpret
it; core types will not gain Ansible-, SSH-, database-, Kubernetes-, or
Terraform-specific fields.

## Mapping the initial Ansible use case

Given these files:

```text
inventories/mariadb/host-123.yml
inventories/mariadb/host-124.yml

playbooks/mariadb/backup.yml
playbooks/mariadb/restart.yml
```

the conceptual Crono model is:

```text
Namespace: mariadb

Targets:
    host-123
    host-124

Jobs:
    mariadb/backup
    mariadb/restart
```

An execution request can select `mariadb/backup`, target
`mariadb/host-123`, and inputs such as `full: true`. The inventory path is
executor-specific target data interpreted later by the execution layer. The
inventory file is not a Job, and Crono does not parse it in the core domain.

The same model supports Jobs and Targets such as
`kubernetes/restart-deployment` with `kubernetes/prod-cluster`,
`terraform/plan` with `terraform/network-prod`, and `postgres/backup` with
`postgres/pg-cluster-01` without changing core types.

## Target reproducibility

Mutable target configuration cannot safely be attached directly to Runs. For
example, a target might refer to DB-A when a Run is created and later be edited
to refer to DB-B before a retry. The retry must still be able to recover the
exact definition selected by the original Run.

`TargetVersion` is therefore a required future invariant, but is intentionally
not implemented in the current identity-only model. There is no target
configuration to version yet, and a shell version type would misleadingly
suggest reproducibility is enforced. The current no-op Run pins the Target
identity only. Before execution and retry semantics are finalized, a Run must
instead pin the equivalent of:

```text
Run
├── job_version_id
├── target_version_id
└── inputs
```

TargetSet versioning or membership snapshot semantics must be resolved as part
of the same design. No current persistence fields or fan-out behavior are
implied by this document.

## Public client shape

The browser information architecture follows the domain rather than an
executor:

```text
Namespaces
├── MariaDB
│   ├── Jobs
│   │   ├── Backup
│   │   ├── Restart
│   │   └── Upgrade
│   ├── Targets
│   │   ├── host-123
│   │   ├── host-124
│   │   └── host-125
│   └── Target Sets
│       ├── mariadb-prod
│       └── mariadb-stage
├── PostgreSQL
└── Patroni
```

The eventual workflow is `Namespace -> Job -> Target or TargetSet -> Inputs ->
Run`. Both the browser and CLI must express that workflow through the same
public `crono-server` API and server-side application logic.

Implemented public resource collections are:

```text
/api/v1/namespaces
/api/v1/namespaces/:namespace/jobs
/api/v1/namespaces/:namespace/targets
/api/v1/runs
```

Target Set routes remain deferred with their snapshot and fan-out semantics.

Potential CLI commands are:

```sh
crono namespace list
crono namespace show mariadb

crono job list --namespace mariadb
crono job show mariadb/backup

crono target list --namespace mariadb
crono target show mariadb/host-123

crono target-set list --namespace mariadb
crono target-set show mariadb/mariadb-prod

crono run mariadb/backup --target mariadb/host-123
crono run mariadb/backup --target-set mariadb/mariadb-prod
```

The CLI commands still document intended organization rather than an
implemented CLI transport. The browser uses the implemented HTTP resources via
the transport-only `crono-api` crate. Neither public client may depend on server
domain modules or bypass the HTTPS API.
