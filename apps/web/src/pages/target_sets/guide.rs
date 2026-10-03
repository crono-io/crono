//! Read-only Target Set examples alongside the resource editor.
//!
//! A greeting illustrates one Run per member and a Target overriding a shared
//! input. The deployment example connects the same model to Job arguments and
//! Target suffixes. All values are illustrative; opening examples never writes
//! resources, starts execution, or replaces an entered draft.

use leptos::prelude::*;
use leptos_router::components::A;

const SUMMARY_CLASS: &str = "cursor-pointer rounded-md px-4 py-3 text-sm font-semibold text-crono-text hover:bg-zinc-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary";
const INLINE_LINK_CLASS: &str = "rounded-sm font-medium text-crono-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary";

/// Show the authoring sequence and where a saved group is used for execution.
#[component]
pub(super) fn TargetSetSteps() -> impl IntoView {
    view! {
        <section aria-label="How to use a Target Set" class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
            <ol class="grid gap-4 text-sm sm:grid-cols-3 sm:gap-6">
                <li>
                    <p class="font-semibold text-crono-text">"1. Choose Targets"</p>
                    <p class="mt-1 text-crono-muted">"Select existing Targets from one Namespace. Each member supplies its own arguments and inputs."</p>
                </li>
                <li>
                    <p class="font-semibold text-crono-text">"2. Add shared inputs"</p>
                    <p class="mt-1 text-crono-muted">"Name the group and add values every member can use. A Target can override a shared value."</p>
                </li>
                <li>
                    <p class="font-semibold text-crono-text">"3. Run a Job"</p>
                    <p class="mt-1 text-crono-muted">"Save, then choose the set in "<A href="/runs/new" attr:class=INLINE_LINK_CLASS>"Run Job"</A>" or "<A href="/schedules" attr:class=INLINE_LINK_CLASS>"Schedules"</A>". One Job request creates one Run per member."</p>
                </li>
            </ol>
        </section>
    }
}

/// Keep example commands, inputs, and precedence separate from editable fields.
#[component]
pub(super) fn TargetSetGuide() -> impl IntoView {
    view! {
        <aside aria-labelledby="target-set-guide-title" class="min-w-0 rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
            <h2 id="target-set-guide-title" class="font-semibold text-crono-text">"See how it works"</h2>
            <p class="mt-2 text-sm text-crono-muted">"Illustrative examples. Create your own Jobs and Targets to try them."</p>
            <div class="mt-5 space-y-3">
                <GreetingExample />
                <DeploymentExample />
                <InputPrecedence />
            </div>
            <p class="mt-4 text-xs text-crono-muted">"Editing membership or inputs affects future Runs. Existing Runs keep the values they were created with."</p>
        </aside>
    }
}

/// Render a wrap-safe, explicitly labeled example value rather than an editor.
#[component]
fn ExampleValue(label: &'static str, value: &'static str) -> impl IntoView {
    view! {
        <div>
            <dt class="text-xs font-medium text-crono-muted">{label}</dt>
            <dd class="mt-1 whitespace-pre-wrap break-words font-mono text-xs leading-5 text-crono-text">{value}</dd>
        </div>
    }
}

/// Demonstrate fan-out and a per-Target override with a simple process Job.
#[component]
fn GreetingExample() -> impl IntoView {
    view! {
        <details id="target-set-greeting-example" open class="rounded-lg border border-crono-border">
            <summary class=SUMMARY_CLASS>"Simple example: greet two people"</summary>
            <div class="space-y-4 border-t border-crono-border p-4 text-sm">
                <p class="text-crono-muted">"The Job decides what to run. Each Target supplies a person's name. The Target Set supplies a common greeting."</p>
                <dl class="space-y-3">
                    <ExampleValue label="Process Job: greet" value="Executable: /bin/echo\nArguments: [\"{{ greeting }}\"]\nDefault inputs: {\"greeting\":\"Hi\"}" />
                    <ExampleValue label="Target: ada" value="Arguments: [\"{{ name }}\"]\nInputs: {\"name\":\"Ada\"}" />
                    <ExampleValue label="Target: ben" value="Arguments: [\"{{ name }}\"]\nInputs: {\"name\":\"Ben\",\"greeting\":\"Welcome\"}" />
                    <ExampleValue label="Target Set: greeting-team" value="Members: ada, ben\nShared inputs: {\"greeting\":\"Hello\"}" />
                </dl>
                <div class="rounded-md bg-crono-primary-soft p-3">
                    <p class="font-medium text-crono-text">"Run greet on greeting-team → 2 separate Runs"</p>
                    <dl class="mt-3 space-y-3">
                        <ExampleValue label="Run for ada" value="/bin/echo Hello Ada" />
                        <ExampleValue label="Run for ben" value="/bin/echo Welcome Ben" />
                    </dl>
                </div>
                <p class="text-xs text-crono-muted">"Hello replaces the Job's Hi. Ben's Welcome overrides the shared Hello only for his Run. Each Target's arguments follow the Job's arguments."</p>
                <p class="text-xs text-crono-muted">"To try it, create these resources in the same Namespace. In Run Job, select greet, choose Target Set, then select greeting-team."</p>
            </div>
        </details>
    }
}

/// Connect shared release configuration to two destination-specific argv suffixes.
#[component]
fn DeploymentExample() -> impl IntoView {
    view! {
        <details id="target-set-deployment-example" class="rounded-lg border border-crono-border">
            <summary class=SUMMARY_CLASS>"Deployment example: two web hosts"</summary>
            <div class="space-y-4 border-t border-crono-border p-4 text-sm">
                <p class="text-crono-muted">"Keep the release version in one place while each Target selects its own inventory host."</p>
                <dl class="space-y-3">
                    <ExampleValue label="Process Job: deploy-web" value="Executable: /usr/bin/ansible-playbook\nArguments: [\"-i\", \"/etc/ansible/hosts\", \"/srv/playbooks/deploy.yml\", \"--extra-vars\", \"version={{ version }}\"]" />
                    <ExampleValue label="Target: web-01" value="Arguments: [\"--limit\", \"{{ host }}\"]\nInputs: {\"host\":\"web01.example.com\"}" />
                    <ExampleValue label="Target: web-02" value="Arguments: [\"--limit\", \"{{ host }}\"]\nInputs: {\"host\":\"web02.example.com\"}" />
                    <ExampleValue label="Target Set: web-fleet" value="Members: web-01, web-02\nShared inputs: {\"version\":\"2.4.0\"}" />
                </dl>
                <div class="rounded-md bg-crono-primary-soft p-3">
                    <p class="font-medium text-crono-text">"Run deploy-web on web-fleet → 2 separate Runs"</p>
                    <p class="mt-2 text-xs text-crono-muted">"Both use version 2.4.0. One adds "<code class="break-words font-mono">"--limit web01.example.com"</code>"; the other adds "<code class="break-words font-mono">"--limit web02.example.com"</code>"."</p>
                </div>
                <p class="text-xs text-crono-muted">"The worker running this Job needs Ansible, the inventory and playbook files, and access to the hosts. The Job's program handles the connection to each host."</p>
            </div>
        </details>
    }
}

/// Explain the authoritative input-layer order and the effect of matching keys.
#[component]
fn InputPrecedence() -> impl IntoView {
    view! {
        <details id="target-set-input-precedence" class="rounded-lg border border-crono-border">
            <summary class=SUMMARY_CLASS>"Which input value wins?"</summary>
            <div class="space-y-3 border-t border-crono-border p-4 text-sm text-crono-muted">
                <p>"Inputs are applied in this order. Later layers override matching values:"</p>
                <ol class="list-inside list-decimal space-y-2 text-crono-text">
                    <li>"Job default inputs"</li>
                    <li>"Target Set shared inputs"</li>
                    <li>"Target inputs"</li>
                    <li>"Schedule or manual Run inputs"</li>
                </ol>
                <p class="text-xs">"Nested objects merge by key. Arrays, scalar values, and null replace the earlier value."</p>
                <p class="text-xs">"In the greeting example, manual Run inputs "<code class="break-all font-mono">"{\"greeting\":\"Good morning\"}"</code>" make both Runs use Good morning, including Ben's."</p>
            </div>
        </details>
    }
}
