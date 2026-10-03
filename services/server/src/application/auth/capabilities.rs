//! Stable permission identifiers and their typed resource and assignment boundaries.
//!
//! This registry is the version-one vocabulary shared with IAM adapters. Roles are
//! deliberately absent: a provider maps verified authority into these permissions,
//! and the policy still checks the actual resource. Identifiers are explicit so
//! Rust renaming cannot silently change the external contract.

use super::{ResourceKind, ResourceScope};

/// Assignment boundary accepted by a permission, independent of IAM role names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionScope {
    Global,
    Namespace,
}

/// One published permission and the resources on which it can be requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionDefinition {
    pub capability: Capability,
    pub identifier: &'static str,
    pub scope: PermissionScope,
    pub resources: &'static [ResourceKind],
    pub description: &'static str,
}

macro_rules! permissions {
    ($( $variant:ident => ($identifier:literal, $scope:ident, [$($kind:ident),+], $description:literal) ),+ $(,)?) => {
        /// Stable application permissions, with explicit external identifiers.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Capability {
            $(#[doc = $description] $variant,)+
        }

        /// Complete catalog used for development grants and schema consistency.
        pub const ALL_CAPABILITIES: &[Capability] = &[$(Capability::$variant,)+];

        impl Capability {
            /// Return this permission's immutable identifier and authorization boundaries.
            #[must_use]
            pub const fn definition(self) -> PermissionDefinition {
                match self {
                    $(Self::$variant => PermissionDefinition {
                        capability: self,
                        identifier: $identifier,
                        scope: PermissionScope::$scope,
                        resources: &[$(ResourceKind::$kind),+],
                        description: $description,
                    },)+
                }
            }
        }
    };
}

permissions! {
    NamespaceCreate => ("crono.namespace.create", Global, [ControlPlane], "Create a Namespace; does not assign access to it."),
    NamespaceRead => ("crono.namespace.read", Namespace, [Namespace], "Read Namespace metadata; does not expose workload counts."),
    NamespaceDelete => ("crono.namespace.delete", Namespace, [Namespace], "Delete an empty Namespace subject to bootstrap protections."),
    QueueCreate => ("crono.queue.create", Global, [ControlPlane], "Create a global worker Queue."),
    QueueRead => ("crono.queue.read", Global, [ControlPlane, Queue], "Read global Queues, including selecting a Queue for a Job."),
    QueueUpdate => ("crono.queue.update", Global, [Queue], "Edit or enable a global Queue."),
    QueueDelete => ("crono.queue.delete", Global, [Queue], "Delete an unused non-system Queue."),
    JobCreate => ("crono.job.create", Namespace, [Namespace], "Create executable Job definitions in a Namespace."),
    JobRead => ("crono.job.read", Namespace, [Namespace, Job], "Read Job definitions and execution configuration."),
    JobUpdate => ("crono.job.update", Namespace, [Job], "Replace executable configuration for future executions."),
    JobExecute => ("crono.job.execute", Namespace, [Job], "Execute a Job; Run creation and selected Target use are checked separately."),
    TargetCreate => ("crono.target.create", Namespace, [Namespace], "Create a Target's arguments and inputs."),
    TargetRead => ("crono.target.read", Namespace, [Namespace, Target], "Read Target arguments and inputs."),
    TargetUpdate => ("crono.target.update", Namespace, [Target], "Replace a Target's arguments and inputs for future executions."),
    TargetDelete => ("crono.target.delete", Namespace, [Target], "Delete an unused Target subject to starter protections."),
    TargetUse => ("crono.target.use", Namespace, [Target], "Use a Target's arguments and inputs for execution."),
    TargetSetCreate => ("crono.target_set.create", Namespace, [Namespace], "Create a Target Set; member Target reads are checked separately."),
    TargetSetRead => ("crono.target_set.read", Namespace, [Namespace, TargetSet], "Read Target Set configuration and membership."),
    TargetSetUpdate => ("crono.target_set.update", Namespace, [TargetSet], "Replace Target Set membership and inputs, affecting future scheduled executions."),
    TargetSetUse => ("crono.target_set.use", Namespace, [TargetSet], "Use a Target Set; each selected Target requires its own use grant."),
    ScheduleCreate => ("crono.schedule.create", Namespace, [Namespace], "Create recurring or deferred execution; underlying execution grants are required."),
    ScheduleRead => ("crono.schedule.read", Namespace, [Namespace, Schedule], "Read Schedule timing, references, and policy."),
    ScheduleUpdate => ("crono.schedule.update", Namespace, [Schedule], "Enable or disable a Schedule; enabling requires execution grants."),
    WorkflowCreate => ("crono.workflow.create", Namespace, [Namespace], "Create a Workflow graph; referenced Job reads are checked separately."),
    WorkflowRead => ("crono.workflow.read", Namespace, [Namespace, Workflow], "Read Workflow graphs in visible Namespaces."),
    WorkflowUpdate => ("crono.workflow.update", Namespace, [Workflow], "Replace a Workflow graph at its expected revision."),
    WorkflowDelete => ("crono.workflow.delete", Namespace, [Workflow], "Delete a Workflow subject to invocation references."),
    WorkflowExecute => ("crono.workflow.execute", Namespace, [Workflow], "Launch a Workflow; all underlying execution grants are checked separately."),
    WorkflowRunRead => ("crono.workflow_run.read", Namespace, [Workflow, WorkflowRun], "Read invocation graphs and child Run identities; output needs Run read."),
    WorkflowRunCancel => ("crono.workflow_run.cancel", Namespace, [WorkflowRun], "Cancel pending Workflow work; invocation read is also required, with no process-kill authority."),
    RunCreate => ("crono.run.create", Namespace, [Namespace], "Commit execution intent in a Namespace; Job and Target grants remain required."),
    RunRead => ("crono.run.read", Namespace, [Run], "Read Run history, Attempt output, and lifecycle events."),
    WorkerRead => ("crono.worker.read", Global, [ControlPlane], "Read global worker presence and bounded diagnostics."),
    MonitorRead => ("crono.monitor.read", Global, [ControlPlane], "Read global operational and database monitoring."),
    OverviewRead => ("crono.overview.read", Namespace, [Namespace], "Read aggregate workload counts for assigned Namespaces."),
}

impl Capability {
    /// Parse an exact version-one identifier; unknown names never imply authority.
    #[must_use]
    pub fn from_permission_id(identifier: &str) -> Option<Self> {
        ALL_CAPABILITIES
            .iter()
            .copied()
            .find(|capability| capability.definition().identifier == identifier)
    }

    /// Check only structural resource compatibility; this grants no caller permission.
    #[must_use]
    pub fn accepts_resource(self, resource: &ResourceScope) -> bool {
        self.definition().resources.contains(&resource.kind())
    }
}
