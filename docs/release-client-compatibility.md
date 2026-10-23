# Release client compatibility investigation — 2026-10-23

The two-service sample failed at `POST /v2/projects/liara-two-service-demo-web/releases` with HTTP 500. Read-only account-scoped checks confirmed that its app and managed PostgreSQL database existed and release history was empty. The failed request had already uploaded the approved source and configured the app environment.

## Observed provider behavior

The Rust client previously sent no User-Agent. Adding a generic application User-Agent allowed release submission and the remote Docker build, but that release returned 502 and its application log said `Please upgrade your client using: npm install -g @liara/cli`. A build marked READY was therefore insufficient evidence that the actual app worked.

The fixed client sends `@liara/cli/9.6.0 (compatible; liara-launch/0.1.0)`. It preserves the provider's versioned CLI compatibility token and explicitly identifies this application. The [official CLI's Base class](https://github.com/liara-cloud/cli/blob/14ec35f/src/base.ts) sends its generated User-Agent on API requests. [Oclif's configuration](https://github.com/oclif/core/blob/3.27.0/src/config/config.ts) constructs that token from the package name and version; the audited Liara CLI package was version 9.6.0. No CLI process or Node sidecar is required by the desktop app.

The server-side compatibility behavior is **observed integration behavior**, not a published OpenAPI header requirement. The API schema does not explain the initial 500 or the runtime compatibility rejection. The compatibility version is pinned to the audited CLI contract; future provider changes require reviewing that contract and integration testing rather than blindly changing the token.

## PostgreSQL default database

The account's database detail returned credentials and the private hostname/port but no database name. The old generated URL ended in `/`, which makes common PostgreSQL drivers fall back to a username-derived database. Liara's [Node.js PostgreSQL documentation](https://docs.liara.ir/dbaas/postgresql/how-tos/connect-via-platform/nodejs/) uses the default `postgres` database. Generated PostgreSQL connection values now default to this documented name. An exact previously generated URL missing only this default database suffix is repaired; unrelated explicit connection overrides remain intact. No application source changes are needed.

## Live verification

The user requested repair and deployment of the already-approved sample. Two bounded deployment-configuration repair attempts reused its existing resource IDs and uploaded source. No duplicate app/database, additional disk, plan upgrade or original source edit was performed.

The corrected live release passed:

- Public page: HTTP 200 with the sample's actual Persian interface.
- `/healthz` and `/api/status`: HTTP 200 with `web: ok` and `database: ok`.
- Browser counter: incremented from zero to one and remained one after reloading, confirming the deployed app could read/write PostgreSQL.

Live URL: https://liara-two-service-demo-web.liara.run/

Screenshot: `docs/screenshots/sample-live-verified.png`.

Regression checks cover the actual User-Agent transmitted over a local HTTP connection, the PostgreSQL default/legacy URL repair, preservation of explicit overrides, and suppression of obsolete failure modals after successful recovery. The persisted failed run was continued and now records a verified success; older error events remain part of its history.
