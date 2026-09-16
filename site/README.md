# Seyal documentation site

This directory is the public documentation product for Seyal OSS.

## Audiences

- `User Guide` — task-oriented documentation for people using Seyal.
- `Developer Guide` — contributor orientation and links into authoritative engineering records.

The existing repository directories `docs/architecture`, `docs/specs`, `docs/milestones`, and `docs/engineering` remain source-of-truth engineering documents. Do not duplicate accepted architecture into the site; summarize for orientation and link back to authority.

## Local development

Requires Node.js 22.12 or later.

From the repository root, use the canonical commands:

```sh
make docs          # install docs dependencies and start the local server
make docs-install  # install docs dependencies only
make docs-build    # build the static documentation site
make docs-check    # validate Starlight/Astro content
```

`make docs` is the normal way to view the documentation locally. The development server prints the local URL after startup. Docs dependencies are installed with `npm ci` against the committed `site/package-lock.json`; update the lockfile in the same change when adding or bumping site packages.

Direct `npm` commands inside `site/` remain implementation details of these Make targets and should not become a competing documented workflow.

## Media policy

Images are encouraged when they improve understanding. Prefer diagrams and real screenshots. A screenshot must match a real current UI and must not expose credentials, private/customer data, tokens, private paths, signing identities, or other sensitive material.

Do not use generated concept UI as evidence that a workflow exists. Tutorial videos should be created only after the relevant UI/flow is stable; record the demonstrated Seyal version or commit and retire stale videos deliberately.

## Scope

This site documents the public Seyal OSS product and contributor experience. Do not publish private implementation, account, infrastructure, customer, pricing, entitlement, or internal service details here.
