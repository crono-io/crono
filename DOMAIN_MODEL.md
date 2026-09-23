# Workload domain model

Crono's draft model separates what runs from where it runs, then snapshots both when durable execution intent is created.

```text
Namespace
   +-- Job
   +-- Target
   +-- Schedule -> occurrence

Job + Target + inputs
         |
         v
        Run -> RunAttempt -> Worker
```

A Namespace is the ownership and authorization boundary for names. A Job is a directly editable execution definition containing an executor, queue, executable and arguments, idempotency declaration, and retry policy. A Target contains destination-specific arguments. A Schedule refers to one Job and Target in the same Namespace. Database triggers reject cross-Namespace references even if an adapter is faulty.

This draft deliberately has no JobVersion, TargetVersion, ScheduleVersion, `/api/v1`, or schema-version field in dispatch messages. Editing a catalog object affects only future Runs. Every Run stores an immutable execution snapshot, so already committed work and history do not change when the Job or Target is edited.

## Identities and occurrence uniqueness

Names are lowercase canonical resource names and are unique inside their Namespace. Internal identities are UUIDv7 values. A manual Run uses the request UUID as an idempotency key: replaying the same request and definition returns the existing Run, while reusing the key for different work is a conflict.

A scheduled occurrence is identified by `(schedule_id, scheduled_at)`. PostgreSQL enforces that pair as unique. Scheduler claims improve concurrency, but this constraint is the final correctness boundary during failover or competing scheduler instances.

Each logical Run can have multiple Attempts. Message redelivery for an existing Attempt never allocates another Attempt. Execution retry does: the old Attempt remains immutable audit history and a transaction creates the next Attempt plus its outbox event.

## Schedule state

A Schedule stores either a five-field cron expression with an IANA timezone or a one-shot UTC timestamp. It also persists `enabled`, `next_run_at`, `last_run_at`, misfire policy, catch-up policy and limits, revision, and short scheduler claim data. `next_run_at` is authoritative and indexed; it is not recomputed by scanning every Schedule.

The scheduler transaction validates claim ownership, evaluates one bounded plan, inserts executable or skipped Runs, inserts outbox rows for executable occurrences, advances the cursor, writes audit events, and commits. A rollback leaves none of those changes visible.

## Run and Attempt state

A Run begins at `pending_dispatch`, becomes `queued` only after JetStream acknowledges persistence, and becomes `running` only after a PostgreSQL-backed worker claim. Success and non-retryable failure are terminal. Retryable execution failure uses `retry_wait`; the reconciler later creates another Attempt and returns the Run to `pending_dispatch`. `skipped`, `cancelled`, `dead`, and `unknown` preserve other terminal dispositions explicitly.

An Attempt separately records `pending_dispatch`, `queued`, `running`, and its terminal result, along with the worker, start/heartbeat/lease timestamps, bounded output, exit status, and error. The outbox links one-to-one to an Attempt and records publication attempts independently from execution attempts.

## Leases and idempotency

The worker has no PostgreSQL credentials. It claims, renews, and completes through identity-scoped NATS request/reply subjects handled by the server. Every operation uses conditional state transitions in PostgreSQL. A healthy worker renews the database lease and JetStream ACK deadline independently.

Lease expiry proves only that ownership was lost. It does not prove that a local process stopped or that a remote effect did not happen. Crono automatically creates another Attempt only for a Job declared idempotent and with attempts remaining. Otherwise the Run becomes `unknown`. Resource-specific idempotency keys or fencing must protect external systems when automatic retries are enabled.

## Trust boundary

Only the server accepts public control-plane requests and writes PostgreSQL. Only the server publishes execution dispatch. Workers consume authorized queues and use scoped control subjects; a payload's claimed `worker_id` is not sufficient authentication. Production broker credentials must restrict those subjects so the authenticated identity and subject identity agree.

The process executor does not invoke a shell. Executables must be absolute, arguments remain structured, inputs are passed through a bounded temporary JSON file, and inherited environment is cleared except for an explicit locale/timezone allowlist. This reduces accidental injection but is not a sandbox: a worker process has the privileges and network access of its operating-system identity.
