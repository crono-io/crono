# Workflows

A Workflow is a directed acyclic graph of existing Jobs in one Namespace.
Nodes reference Job UUIDs; edges decide when a node may create its normal Runs.
Workers, execution retries, queues, Attempts, and NATS dispatch use the existing
Crono execution path. Workflow definitions contain references, not Job copies.

```text
Workflow + Target / Target Set + invocation Inputs
                       |
                       v
             immutable WorkflowRun snapshot
                       |
                       v
                 WorkflowNodeRun
                       |
                       v
                 ordinary Run(s)
                       |
                       v
       Attempt -> PostgreSQL outbox -> NATS -> Worker
```

## Dependencies and outcomes

`success` accepts a succeeded predecessor. `failure` accepts failed or unknown
outcomes, treating an ordinary Run's `dead` disposition as failure. `always`
accepts every terminal predecessor, including skipped and cancelled. An unknown
outcome remains visible: following its failure branch does not prove the
original external effect did not happen.

All incoming edges must match: joins use AND semantics. A predecessor executing,
queued, or waiting for an ordinary execution retry leaves its successor pending.
A terminal mismatch skips the successor without creating a Run. Skips propagate
through affected downstream dependencies, so impossible paths do not stay pending
forever. A skipped node can unlock `always`, but neither `success` nor `failure`.

```text
Linear:    A --success--> B --success--> C

                       +--success--> B --success--+
Parallel:  A --success--+                         +--> D
                       +--success--> C --success--+

                         +--success--> verify
Branch:    deploy -------+
                         +--failure--> rollback
```

Roots have no incoming edges and create normal Runs atomically at launch.
Eligible siblings proceed independently; queues and worker concurrency determine
when child Runs execute. Node `running` means children have been created,
including dispatch, queueing, and retries. Node start/end timestamps measure
that orchestration interval; ordinary Run timestamps measure worker execution.

A WorkflowRun finishes after all nodes terminate. It fails if any executed node
failed, became unknown, or was independently cancelled; successful rollback
does not erase an earlier failure. Otherwise it succeeds, including intentional
dependency skips and dry-run skips. Explicit workflow cancellation ultimately
finishes cancelled after active children drain. One failure never aborts the
graph: valid failure/cleanup branches continue until their dependencies resolve.

## Target Sets, Inputs, and immutable history

The launch selection is inherited by every node. A single Target creates exactly
one ordinary Run per eligible WorkflowNodeRun. A Target Set creates one ordinary
Run per member under one normal invocation identity. These children are separate
from DAG dependencies. A node waits for **all** members to terminate: all succeeded
means succeeded; otherwise unknown takes precedence over failed/dead, cancelled,
and skipped. This is a fleet barrier, not a separate DAG per Target.

The existing merge/rendering engine applies:

```text
Job defaults < Target Set inputs < Target inputs < Workflow invocation inputs
```

Inputs are configuration, never a secret store. Launch pins the graph, member
UUIDs, Job execution definitions, Target arguments, merged Inputs, rendered
arguments, and retry policy for all nodes. Subsequent catalog edits affect only
future launches, even when a pending node has not yet created its Run. That Run
receives its prepared immutable snapshot and a unique execution idempotency key.

## API

Create/list definitions at `/api/namespaces/{namespace_id}/workflows`. Read,
replace, or delete at `/api/workflows/{workflow_id}`. Names use Crono's canonical
DNS-1123 labels; edges address names local to the graph. UUIDs remain resource
identities. PUT requires the current `revision`; stale updates return 409.
Node UUIDs identify a definition revision and remain in its invocation snapshots.

For example, POST a graph using existing Job UUIDs:

```json
{
  "name": "postgres-upgrade",
  "description": "Back up, upgrade, then verify or roll back",
  "nodes": [
    {"name": "backup", "job_id": "<backup-job-uuid>"},
    {"name": "upgrade", "job_id": "<upgrade-job-uuid>"},
    {"name": "verify", "job_id": "<verify-job-uuid>"},
    {"name": "rollback", "job_id": "<rollback-job-uuid>"}
  ],
  "edges": [
    {"from": "backup", "to": "upgrade", "condition": "success"},
    {"from": "upgrade", "to": "verify", "condition": "success"},
    {"from": "upgrade", "to": "rollback", "condition": "failure"}
  ]
}
```

Start/list invocations at `/api/workflows/{workflow_id}/runs`. POST accepts
`request_id`, the existing `target` selection (`kind` = `target` or `target_set`,
with its UUID `id`), and optional `inputs`. A fresh request returns 201. Repeating
the same Workflow, selection, and Inputs with the same request UUID returns its
existing snapshot/progress with 200, including after catalog edits. Reusing the
UUID for different work returns 409. Inputs use PostgreSQL JSON value equality,
including equivalent numeric representations; storage normalization never creates
a false conflict. Node invocation UUIDs separately identify
normal per-Target Run batches.

Read `/api/workflow-runs/{workflow_run_id}` for the immutable graph/revision,
selection/Inputs, overall state, cancellation flag, node states, per-target Run
IDs, and RFC 3339 timestamps. Definitions use an `after` name cursor; invocation
history uses a newest-first `before` UUID cursor. Both use existing 1–100 page
limits. Clients can reconstruct the graph and follow normal Run/Attempt routes
for output and worker details. The web interface exposes these contracts through
a structured editor and a read-only dependency graph.

POST `/api/workflow-runs/{workflow_run_id}/cancel` atomically stops pending
nodes. Active children drain because Crono does not yet expose safe ordinary Run
cancellation. While they remain active, the invocation stays `running` with
`cancellation_requested`; no new node starts after that flag commits. Repeated
cancellation is safe and finished invocations remain unchanged. Deleting a
Workflow with invocation history returns 409; history is never cascaded away.

## Validation and authorization

Definitions require 1–64 nodes and at most 256 edges. Missing/cross-Namespace
Jobs, missing endpoints, self edges, duplicate endpoint pairs (even with
different conditions), duplicate node names, and cycles are rejected. Kahn's
topological validation runs before persistence. PostgreSQL additionally enforces
ownership, names, uniqueness, bounds, and acyclicity through foreign keys and
deferred graph constraints. Failed replacements preserve the preceding graph.

Launch requires `WorkflowExecute`, every referenced `JobExecute`, `TargetSetUse`
where selected, every member's `TargetUse`, and Namespace `RunCreate`. Catalog
creation/replacement checks `WorkflowCreate`/`WorkflowUpdate` and referenced
`JobRead`. Reads require `WorkflowRead` or `WorkflowRunRead` plus server-derived
Namespace visibility; hidden graphs/history return 404. Deletion/cancellation
have their own typed capabilities. Child output independently requires `RunRead`.

The launch commits authority for the complete pinned execution intent, like
existing Runs and Schedules. Background evaluation grants no new permissions and
does not impersonate callers. PostgreSQL checks the authorized graph revision
and exact Target membership to reject concurrent changes introducing unapproved
work. Preparation is bounded to 4,096 child executions and 8 MiB of snapshot JSON
per invocation; oversized launches fail atomically with 400 and a message naming
those limits. Every replay, including a concurrent launch discovered inside the
transaction, rechecks permissions on the historical Jobs and pinned Targets
before returning graph or child Run identifiers.

## Durability and scheduling

PostgreSQL owns definitions, immutable invocation snapshots, node state, prepared
executions, and completion events. A trigger on ordinary terminal Run updates
commits the event with the outcome, covering completion, dispatch expiry, and
ambiguous lease expiry. The existing reconciler consumes a bounded queue of
affected invocations. It aggregates affected nodes, reevaluates their downstream
dependencies, and recursively propagates skips; it does not rescan every graph.
Dependency progress follows the existing five-second reconciliation cadence;
completion events survive downtime and resume when a server returns.

Invocation row locks serialize evaluation and cancellation across servers.
Node/Target uniqueness and normal invocation identities prevent duplicate Runs.
All node invocation UUIDs are reserved at launch; manual Run requests cannot
adopt them, even while the node is pending.
Downstream Run/Attempt/outbox creation and completion-event consumption commit
together. Before-commit crashes change nothing; after-commit restarts continue
from the same records. NATS outages leave ordinary outbox entries pending, and
worker restarts use existing lease/retry rules. Workers and dispatch subjects
have no workflow-specific protocol.

Manual launch comes first. Existing Schedules still reference Jobs; occurrence
cursors, per-Target uniqueness, and misfire handling remain unchanged. Distinct
typed Workflow IDs and idempotent invocation IDs provide the future adapter
boundary: a scheduled-workload sum type can select Job or Workflow without
changing dependency evaluation or execution. There are no loops, dynamic nodes,
expressions, OR/XOR joins, sub-workflows, new retries, timers, approvals, or secret
management in this version.

## Web interface

Open Execution → Workflows → All Workflows (`/workflows`) and select a Namespace.
The bounded catalog shows Job and dependency counts, update times, and Open,
Run, Edit, and Delete actions. Create Workflow (`/workflows/new`) selects existing
Jobs in that Namespace; each Job has a unique canonical node name. Changing the
Namespace clears Job selections rather than carrying incompatible references.

Add dependencies using From Job, On success / On failure / Always, and To Job.
The live preview uses fixed-size cards in deterministic layers: parallel Jobs
sit beside each other and joins sit after every predecessor. Arrows include
condition text and patterns as well as color. The form is the editor; the graph
is a scrollable visualization, with structured information available alongside
it. Self dependencies and duplicate endpoint pairs are rejected locally; the
server remains authoritative for cycles and complete validity. API validation
errors stay next to the editor and preserve the draft. Removing a Job with
connected dependencies requires confirmation before removing those draft edges.

Definition details (`/workflows/{id}`) show the graph, Job links, dependencies,
and paged invocation history. Editing (`/workflows/{id}/edit`) sends the loaded
revision and displays unsaved changes. Conflicts preserve the draft. Reload latest opens a confirmation and a copyable
JSON draft; only explicit Discard draft and reload replaces it with the current
server definition and revision after a successful read. A failed reload retains
the draft and its copyable JSON with an actionable error. Job options refresh
alongside the definition; UUID fallback labels preserve references while the
separately authorized catalog is stale or unavailable. Edits affect
future launches only. Delete uses a confirmation dialog and displays the
server's history guard; no frontend cascading deletion occurs.

Run Workflow (`/workflows/{id}/run`) uses the ordinary Target / Target Set and
JSON invocation input controls. The summary names the Workflow, selected
destination, and Job count. An unchanged launch retry reuses its request UUID,
so a lost response does not create duplicate execution. Leaving the launch page
does not cancel an already submitted request; a late response cannot redirect
the operator away from their chosen page. Successful launch opens
`/workflow-runs/{id}`. No execution rules or provider credentials are interpreted
in the browser.

The invocation page uses the immutable launch graph, counts actual node states,
and labels each Job's state directly on the graph and in a structured table.
Skipped branches are expected behavior; a terminal dependency mismatch is
explained using the persisted predecessor result. Every associated per-target
Run links to the ordinary Run detail page for Attempts, output and timeline.
Optional Job or destination listing failures fall back to resource UUIDs rather
than blocking access to an otherwise authorized Workflow snapshot.

Active invocations refresh every five seconds, with at most one status request
in flight. Terminal results stop automatic refresh; leaving the page clears the
timer. Manual Refresh remains available. A failed refresh retains the last good
snapshot with an explicit stale-data warning. Cancellation requires confirmation
and prevents future Jobs from starting; active child Runs finish normally under
the current API semantics. The Workflow becomes cancelled after they drain.
