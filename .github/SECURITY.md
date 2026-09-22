# Security Policy

## Supported Versions

Crono has not published a supported release yet. During development, security
fixes are applied only to the latest revision of the default branch. Before
reporting a problem, please confirm that it is reproducible there and include
the commit identifier you tested.

This policy will be updated with supported version ranges when Crono begins
publishing releases.

## Reporting a Vulnerability

Please do not open a public GitHub issue or discussion for a suspected
security vulnerability.

Report vulnerabilities privately by emailing
[nbari@tequila.io](mailto:nbari@tequila.io). Include, when possible:

- A description of the vulnerability and its potential impact
- The affected Crono component, commit identifier, and platform
- Steps to reproduce the issue or a minimal proof of concept
- Whether the issue involves the server API, authentication or authorization,
  worker execution, messaging, database access, a client, or telemetry export
- Any suggested mitigation or fix

You can expect an initial response within 48 hours and a status update within
seven days. If the report is accepted, the maintainer will coordinate a fix
and release timeline based on its severity and complexity. If it is declined,
the response will explain why it is not considered a vulnerability.

Please keep the report confidential until a fixed release is available or a
disclosure timeline has been agreed upon.

## Scope

Reports about vulnerabilities in Crono or in the way it uses its dependencies
are in scope. This includes the server, worker, CLI, web client, shared crates,
and their PostgreSQL, NATS, HTTP, and telemetry trust boundaries. General
support questions and vulnerabilities that only affect an upstream dependency
should be reported to the relevant upstream project unless Crono uses the
dependency in an exploitable way.
