# Workload domain model

Crono's draft model separates what runs from where it runs, then snapshots both when durable execution intent is created.

```text
Namespace
   +-- Job
   +-- Target
   +-- Target Set --> Target membership
   +-- Schedule -> occurrence
   +-- Workflow -> Job nodes + dependency edges -> WorkflowRun

Job + Target + inputs
         |
         v
        Run -> RunAttempt -> Worker

Queue (global) -> Job
       +-------> Worker
```

A Namespace is the ownership and authorization boundary for workload names. A
Queue is a global worker-pool resource. A Job stores the Queue UUID alongside
its executor, executable and arguments, idempotency declaration, and retry
policy. A Target contains destination-specific arguments. A Target Set is a
non-empty named selection of Targets in the same Namespace. A Schedule refers
to one Job and Target in the same Namespace. Database constraints reject
missing Queues and cross-Namespace references even if an adapter is faulty.

This draft deliberately has no JobVersion, TargetVersion, ScheduleVersion, `/api/v1`, or schema-version field in dispatch messages. Editing a catalog object affects only future Runs. Every Run stores an immutable execution snapshot, so already committed work and history do not change when the Job or Target is edited.

## Identities and occurrence uniqueness

UUIDv7 values are immutable resource identities and every relationship stores
those UUIDs. Names are stable lookup and display identifiers, not foreign keys.
Namespace, Queue, Job, Target, Target Set, Schedule, and Workflow names use one
DNS-1123 label rule: 1 through 63 ASCII lowercase letters, digits, or hyphens, with an
alphanumeric first and last character. Writes trim surrounding Unicode whitespace
before validating this canonical value; invalid characters and uppercase letters
remain errors. Names are unique inside their owning Namespace; Namespace
and Queue names are unique globally. Disabling a Queue prevents new Job
assignments while existing Jobs, Runs, and workers can drain. Deletion succeeds
only when no durable relationship references the Queue. The bootstrap `default`
Queue is system-managed: its description may change, but it remains enabled
and cannot be renamed or deleted because the worker CLI uses it as its default.

A manual Run uses the request UUID as an idempotency key: replaying the same request and definition returns the existing Run, while reusing the key for different work is a conflict. Inputs compare by PostgreSQL JSON value equality, preserving numeric precision and tolerating storage normalization of exponent notation. Manual Run and Schedule requests refer to Jobs and Targets by UUID, while API responses also include derived qualified names for display.

A scheduled occurrence is identified by `(schedule_id, scheduled_at)`. PostgreSQL enforces that pair as unique. Scheduler claims improve concurrency, but this constraint is the final correctness boundary during failover or competing scheduler instances.

Each logical Run can have multiple Attempts. Message redelivery for an existing Attempt never allocates another Attempt. Execution retry does: the old Attempt remains immutable audit history and a transaction creates the next Attempt plus its outbox event.

## Structured input normalization

Write requests trim surrounding Unicode whitespace from resource names,
Workflow node names and dependency endpoints, executable and interpreter paths,
cron expressions, timezones, and timestamp strings before validation. The web
client also trims these fields on blur and submission, and trims numeric fields
before parsing. Name length limits apply to the canonical value after trimming.
Stored names and API responses retain their canonical constraints; only write
inputs accept padding.
For example, `/usr/bin/echo ` becomes `/usr/bin/echo`; whitespace inside a path
or expression stays intact. Crono does not change case or remove quotes.

Scripts, individual arguments and argument templates, JSON values, descriptions,
and credentials retain their exact contents. These values may intentionally
contain whitespace and are not structured identifiers. Workers execute the
stored snapshot without applying new normalization rules. Existing Job
definitions can be edited and saved to correct padded paths; no migration
rewrites existing definitions, Runs, or WorkflowRun snapshots.

## Workflow state

Workflows are bounded DAGs of existing Jobs, with success/failure/always edges
and AND joins. Each WorkflowRun snapshots its graph and execution context;
eligible node invocations create ordinary Runs and preserve Target Set fan-out.
PostgreSQL completion events drive dependency decisions through the existing
reconciler. Workers and NATS remain unaware of the graph. See
[WORKFLOWS.md](WORKFLOWS.md) for execution, cancellation, and API details.

## Schedule state

A Schedule stores either a five-field cron expression with an IANA timezone or a one-shot UTC timestamp. It also persists `enabled`, `next_run_at`, `last_run_at`, misfire policy, catch-up policy and limits, revision, and short scheduler claim data. `next_run_at` is authoritative and indexed; it is not recomputed by scanning every Schedule.

The scheduler transaction validates claim ownership, evaluates one bounded plan, inserts executable or skipped Runs, inserts outbox rows for executable occurrences, advances the cursor, writes audit events, and commits. A rollback leaves none of those changes visible.

## Run and Attempt state

A Run begins at `pending_dispatch`, becomes `queued` only after JetStream acknowledges persistence, and becomes `running` only after a PostgreSQL-backed worker claim. Success and non-retryable failure are terminal. Retryable execution failure uses `retry_wait`; the reconciler later creates another Attempt and returns the Run to `pending_dispatch`. `skipped`, `cancelled`, `dead`, and `unknown` preserve other terminal dispositions explicitly.

An Attempt separately records `pending_dispatch`, `queued`, `running`, and its terminal result, along with the worker, start/heartbeat/lease timestamps, bounded output, exit status, and error. The outbox links one-to-one to an Attempt and records publication attempts independently from execution attempts.

## Leases and idempotency

The worker has no PostgreSQL credentials. At startup it resolves its configured
Queue name through a server-owned NATS request/reply boundary, then subscribes
to a UUID-based dispatch subject. It claims, renews, and completes through
identity-scoped NATS request/reply subjects handled by the server. Every
operation uses conditional state transitions in PostgreSQL. A healthy worker
renews the database lease and JetStream ACK deadline independently.

Lease expiry proves only that ownership was lost. It does not prove that a local process stopped or that a remote effect did not happen. Crono automatically creates another Attempt only for a Job declared idempotent and with attempts remaining. Otherwise the Run becomes `unknown`. Resource-specific idempotency keys or fencing must protect external systems when automatic retries are enabled.

## Trust boundary

Only the server accepts public control-plane requests and writes PostgreSQL.
Only the server publishes execution dispatch. Workers resolve human-readable
Queue names but consume UUID-addressed Queue subjects and use scoped control
subjects; a payload's claimed `worker_id` is not sufficient authentication.
Production broker credentials must restrict those subjects so the authenticated
identity and subject identity agree.

The process executor does not invoke a shell. Executables must be absolute, arguments remain structured, inputs are passed through a bounded temporary JSON file, and inherited environment is cleared except for an explicit locale/timezone allowlist. This reduces accidental injection but is not a sandbox: a worker process has the privileges and network access of its operating-system identity.
