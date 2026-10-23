# Liara Launch

Liara Launch is a Linux desktop deployment assistant built with Tauri 2, React, TypeScript, and Rust. It scans a local project, combines deterministic findings with an optional OpenAI compatible agent, presents a deployment plan for review, and can deploy supported services through Liara APIs. It supports English and Persian (RTL with Yekan Bakh), keeps credentials in Linux Secret Service when available, and records resumable runs in SQLite.

## Requirements

- Linux with GTK/WebKitGTK development packages. On Debian or Ubuntu, use the Tauri 2 Linux prerequisites listed at <https://v2.tauri.app/start/prerequisites/>.
- Node.js 20.19+ or 22.12+, npm, and Rust stable.
- A graphical X11 or Wayland session to open the native window.
- A Liara API token for live account and deployment actions. An OpenAI compatible provider is optional for deterministic inspection and plan creation. An optional Team ID scopes Liara API requests to a team; leave it blank for a personal account. The connect screen includes a short Team ID tutorial.

## Run locally

```sh
npm install
cp .env.example .env
# Set OPENAI_BASE_URL, OPENAI_MODEL, and OPENAI_API_KEY in .env for agent analysis.
npm run desktop
```

The Rust process loads `.env` from the working directory. Keep it private; `.env` is ignored by Git, and secrets must never use a `VITE_` prefix. The token entered in the app is validated against Liara's current-user endpoint. The app stores it in the desktop keyring when available and keeps it in memory for that session otherwise.

The `desktop` command adds Rust's standard `~/.cargo/bin` directory to `PATH` automatically. If you want to run Cargo commands directly in an existing IDE terminal, run `source "$HOME/.cargo/env"` once in that terminal.

To fully stop development mode, press **Ctrl+C** in the terminal running `npm run desktop`, even if you already closed the app window. A still-running dev session keeps port 1420 occupied, so stop that session before starting another one.

Useful commands:

```sh
npm run build
npm run test:e2e
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features desktop
```

## Demo walkthrough

1. Start `npm run desktop`; choose English or Persian and play the bundled token tutorial.
2. Connect with a Liara token. To test the interface without an account, run `npm run test:e2e`; the Playwright suite uses mocked account and deployment responses and does not contact Liara.
3. Select a local project folder and review the detected stack, evidence, service boundaries, and required environment variable names.
4. Review the suggested plan, choose alternatives or existing compatible resources, provide required secrets, and inspect the consolidated cost and configuration summary.
5. Approve the displayed revision to begin a live deployment. The app persists release identifiers and events, then checks the resulting URL. Do not treat mocked browser checks as a real deployment.

Live deployment can create billable Liara resources. The app requires an explicit plan approval before provisioning and does not perform destructive cleanup automatically.

## Documentation research

The crawl includes 1,762 discovered documentation URLs: 1,692 retrieved successfully and 70 unavailable links. Retrieval timestamps, HTTP failures, page hashes, and coverage are recorded in [the crawl manifest](docs/liara-crawl-manifest.json); normalized operations are in [liara-operations.json](docs/liara-operations.json). The per-operation catalog, evidence levels, and release scope are in [liara-api-capabilities.md](docs/liara-api-capabilities.md). The crawler can reuse its local snapshot with `python3 scripts/crawl_liara.py --offline`.

Catalog entries describe documented contracts, not proof that every operation is available to every account. CLI corroboration is called out separately. Unverified schemas, permissions, pricing, or endpoints are not assumed. Current plan and price availability must be checked against the connected account.

## Current scope

The app focuses on local-folder PaaS deployments and managed dependencies. The API inventory also covers databases, object storage, DNS, domains/TLS, mail, VPS, file management, billing, and AI APIs; inventory coverage does not mean those capabilities have a UI. VPS provisioning, mail administration, registrar purchases, background SRE monitoring, and `.deb` packaging are outside this phase. Docker Compose informs analysis and is not deployed as a Compose runtime. Remote builds are used where supported; model generated local shell commands are not executed.

The frontend test suite uses mock APIs. A real Liara deployment has not been run in this workspace, so live account access, account specific quota, costs, and release behavior remain unverified here.

The deployment assistant now asks project-specific workload questions before planning. The default review shows the recommendation and cost with a deploy button. Choose **تغییر تنظیمات استقرار** / **Change deployment settings** to inspect or edit the proposed resources. See [guided planning](docs/guided-planning.md) for behavior, validation and limitations.

Use the trash icons beside recent projects or deployment history to remove local entries. Recent-project removal preserves the metadata needed by existing deployment history; selecting the folder again restores it to recents. History deletion removes that run's resume record and session credentials. Running deployments must be stopped first. These actions never delete project files or Liara cloud resources; cloud resources can continue to incur charges.
