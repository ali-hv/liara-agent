# On-demand reliability assistant

Open **دستیار سلامت سرویس / Reliability assistant** in the sidebar, select an existing Liara project, and click **شروع بررسی / Start checks**. This feature implements the monitoring, troubleshooting, corrective-action, and coding-agent requirements from `challenge-description.md` and `video-guide.md`. It is not a 24-hour monitoring service.

## What happens

The native Rust process discovers apps and databases from the connected account, including the optional Team ID. Deployment history groups associated web/database resources into one project; other cloud apps can be selected individually. The frontend sends a target ID, not an arbitrary URL or resource definition. The backend refreshes the inventory before starting.

An immediate check collects provider status, CPU/memory summary metrics, recent app logs, and a public HTTPS response. Checks repeat every 30 seconds by default while the native app is open (15–300 seconds configurable). A custom health path such as `/healthz` is optional; the default is `/`. Collection concurrency is limited to four resources. Up to four monitoring sessions can run at once. Stopping cancels in-flight collection and AI analysis; the app does not install a daemon, start on login, or resume network checks automatically after restart. An interrupted session remains available as saved history.

The service cards show current evidence and compact resource graphs. Missing, malformed, old (>180 seconds), or inaccessible metrics remain unknown. CPU and memory pressure require two distinct high measurements. Slow responses require two consecutive slow checks (default >2 seconds). HTTP errors, unexpected provider states, and recent error logs also produce signals. Repeated signals are grouped into incidents. An incident closes only after two checks with positive evidence that its original symptoms cleared; missing data cannot close it.

A rules-based report appears immediately. Each new incident triggers an AI diagnosis using the last three redacted samples. **Analyze evidence** can request another report, including when there is no incident. The AI ranks possible causes, cites signal identifiers, and suggests supported actions. The conversation panel is scoped to the selected monitoring session, so users can ask plain-language questions about its latest redacted evidence without mixing another deployment into the answer. It cannot execute them. Provider/configuration failures leave the evidence report available. Reports are attached to their sample so an older report is visibly distinguished from current measurements.

## Corrections and approval

- **Restart:** one app restart, with an interruption warning and explicit approval.
- **Upgrade:** requires sustained CPU or memory pressure. Selects the least expensive verified compatible larger plan, preserves region/feature bundle and minimum CPU/RAM/disk capacity, and displays old/new monthly estimates including replicas. Disk allocation is checked against the live resource. Slow response alone never authorizes an upgrade.
- **Source fix:** passes incident evidence and redacted logs to the existing scoped coding agent. The user sees exact before/after file contents and reasons. Changes need their own single-use approval, are checked for expiry/stale contents/credential files/symlinks, and are backed up. Applying changes opens a fresh deployment plan using existing resources; a separate deployment approval is still required. Successful redeployments update the monitoring session's source association. An unassociated cloud app needs its local folder associated through the deployment flow first.
- **Investigate:** collect more evidence rather than make a speculative mutation.

Action approval is held in memory for 15 minutes and bound to account, resource, incident, current plan/feature bundle, current release, and target price. Current symptoms, resource identity and pricing are checked again before sending. Deployment and corrective mutations are serialized. No action repeats automatically.

The audit record is saved before the POST. An accepted request is marked **verifying**, not fixed. Up to three follow-up checks test original symptoms, HTTP availability, and the target plan where relevant. Only observed improvement is marked **Recovery verified**; otherwise the result is **Recovery not confirmed**. Monitoring continues independently.

Network failures, ambiguous server errors, or interruption during a mutation leave an unsettled audit record and block another change on that resource. **Check server state** is read-only: it can confirm an applied plan, but the API offers no reliable restart operation ID to prove completion. **Acknowledge uncertainty** requires explicit user acknowledgment, does not claim success, and does not send another cloud request. A new action still needs fresh approval. Failures are shown in a modal and at the top of the page.

## Data and API boundaries

Implementation: [`sre.rs`](../src-tauri/src/sre.rs), native commands in [`desktop.rs`](../src-tauri/src/desktop.rs), structured diagnosis in [`agent.rs`](../src-tauri/src/agent.rs), and [`SreView.tsx`](../src/SreView.tsx). Types live in [`sre-types.ts`](../src/sre-types.ts). Native `sre:update` events update the UI; a two-second local SQLite refresh runs only while sessions are active on the page. Liara checks run in Rust even when another page is open.

SQLite stores account-scoped monitoring snapshots: last 60 samples, 40 incidents and 30 action records per session. Each sample includes at most 30 redacted log lines per service (800 characters per line). Logs are filtered to the interval since the previous check; the first check covers the recent five minutes. Environment values and nested connection passwords fetched from the resource are used for redaction and never persisted in the monitor. Redaction cannot identify every arbitrary secret or personal datum an application might print: avoid logging credentials/personal data at source. The UI discloses that redacted diagnostics may be sent to the configured AI provider.

Verified operations are cataloged in [the API inventory](liara-api-capabilities.md), sourced from [Liara API documentation](https://developers.liara.ir/) and [Liara's OpenAPI introduction](https://docs.liara.ir/references/api/about/). CLI corroboration uses [the official CLI](https://github.com/liara-cloud/cli), including logs and canonical hostname behavior.

| Operation | Contract / interpretation |
| --- | --- |
| `GET /v1/projects`, `GET /v1/databases` | Live inventory and stable resource IDs |
| `GET /v1/projects/{name}`, `GET /v1/databases/{id}` | Status, identity, current plan, capacity context |
| `GET /v1/projects/{name}/metrics/summary`, database equivalent | Documented `cpuUsage`/`memoryUsage` arrays with `[timestamp, value]` tuples. Core rates and byte values are an **observed interpretation**, not units promised by the OpenAPI schema. Replicas are summed and plan limits multiplied by scale. |
| `GET /v1/projects/{name}/logs?since=<epoch>` | Documented app log endpoint; v1 line shape and official CLI's grouped v2 shape are normalized. |
| `GET /v1/me`, app disks, app releases, domains | Current catalog/prices, capacity and approval fingerprints; active custom TLS domain discovery when the default subdomain is disabled |
| `POST /v1/projects/{name}/actions/restart` | App restart, exactly once per approved action |
| `POST /v1/projects/{name}/resize`, database equivalent | Approved plan change; database disk size preserved |

Health probes use only the verified resource's canonical hostname or an active domain belonging to that resource. They require HTTPS, pin resolved public IPs, reject private/loopback/link-local addresses, do not follow redirects, and use bounded timeouts. They measure availability from this desktop, not worldwide availability. HTTP 2xx/3xx is response evidence, not a complete business-function test. A database's provider status/metrics are not a SQL connection test; the app health route can test dependencies if the app implements one.

Database logs/restarts are not exposed by this verified workflow. The app explicitly reports unavailable data. There is no fabricated database restart endpoint, APM trace, request-volume/error-rate telemetry, load generator, arbitrary shell execution, disk deletion, downgrade, or automatic destructive cleanup. API permissions, outages, account quota, balance, and third-party systems can still prevent an action. Plan estimates exclude taxes and variable usage.

## Repeatable validation and demo

```sh
npm run build
npx playwright test
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop
```

Browser tests use simulated API responses and never deploy or restart a real app. Native tests cover measurement normalization, stale/unknown data, pressure thresholds, incident lifecycle, localized findings, approval binding and single use, compatible upgrades, bounded history, interruption, cancellation, redaction and public-health URL boundaries. Existing source-diff and deployment suites cover reviewed file changes and redeployment approvals.

A read-only live API check is opt-in and needs a token already saved through the app's secure-storage option:

```sh
LIARA_SRE_CHECK_TARGET=liara-two-service-demo-web cargo test \
  --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop \
  sre::tests::live_read_only_check -- --ignored --nocapture
```

This test sends GET requests only. It must not be confused with real corrective-action validation. During implementation, the public sample `/`, `/healthz`, and `/api/status` returned 200. The authenticated opt-in check could not run because no saved Liara token was available. No live restart, resize, source edit or redeployment was performed for SRE verification.

For a short challenge demo:

1. Connect the account/team, open Reliability assistant, select the two-service sample, click Start. Expand health-path settings if the app exposes `/healthz`.
2. Show both services, resource charts, redacted logs and API limitations. Switch Persian/English or light/dark if useful.
3. With a deliberately failing **test** app, show the incident, ranked evidence and diagnosis. The automated mock suite provides a repeatable HTTP 500 scenario without changing production resources.
4. Review a restart and cancel it to show that nothing is changed without approval. For a live action, review the concrete resource and cost, approve in the app, and show its separately verified result.
5. Review a proposed source diff; applying it leads to a fresh deployment approval. Show incident history and Stop. Closing the app also stops monitoring.

Record this walkthrough for the challenge submission; no submission video is generated or published by this implementation.
