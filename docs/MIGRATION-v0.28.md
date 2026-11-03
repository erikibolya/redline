# Migrating RedLine to v0.28 (multi-user)

v0.28 adds named accounts, project roles, comments/notifications, an audit trail, optimistic
revision conflicts, personal tokens and scoped agent tokens. It is a **standalone release**: the
data folder is additive, but the way humans and agents authenticate changes.

The legacy server token keeps working as a **full owner + agent credential** during migration.
Do not delete it until every client has moved to a named login, a PAT or an agent token.

## 1. Before upgrading

1. Stop the bridge (`redline-agent`) so no run is in flight.
2. Back up the whole data folder. On holly that is
   `/mnt/user/appdata/redline` (sessions, `token`, and the new files v0.28 will create):
   ```bash
   sudo tar -C /mnt/user/appdata -czf /mnt/user/backups/redline-pre-0.28.tgz redline
   ```
3. Keep a copy of `/mnt/user/appdata/redline/token` somewhere safe. It is the migration owner
   credential and the current bridge credential.
4. Note the compose project / image tag. On holly the project is
   `/boot/config/plugins/compose.manager/projects/redline`; do **not** run `install-unraid.sh`
   there. Pull the new image through the existing Compose project.

## 2. First start

1. Deploy `ghcr.io/erikibolya/redline:browser` (v0.28) and start the `redline` container.
2. Open `https://redline.ibolya.cz/login`.
   - With no `users.json` yet, the page shows **Create the owner account**.
   - Enter the owner username, display name and password, and paste the server token from
     `/mnt/user/appdata/redline/token` (or `docker logs redline | grep -i token`).
   - This creates the single owner and signs you in. The token is only used for this bootstrap;
     it is not stored in the browser.
3. If the page still shows the old dashboard, hard-reload so the new `app.js` is loaded.

The server now has `users.json`, `server.json` and `auth-sessions.json` in the data folder. The
first owner is the only account; no project memberships exist yet.

## 3. Assign people and projects

1. Open **People & access** from the header (owner only).
2. Create each teammate either with a temporary password or with **Create invite link** and send
   them the one-time `/invite/rl_inv_…` URL (7 days, no SMTP).
3. For each project, set the role per person:
   - **Viewer** — reads and comments,
   - **Tester** — creates/edits their own open tasks, requests agent review, cannot submit,
   - **Developer** — reviews a tester request and sends it to the agent.
4. The owner is global and does not need a membership row. **Inbox is owner-only** after v0.28.
   Existing reviews that live in the Inbox stay visible to the owner; move them to a project
   (header project chip, or `PUT /api/sessions/{id}/project`) so members and agents can reach them.
5. New tasks require a project. The picker only offers projects the person can access.

## 4. Move the agent bridge to an agent token

The legacy token still works, so this step can wait. When ready:

1. In **Tokens → Agent tokens**, create one token per bridge:
   - **Agent id** must match the bridge's `REDLINE_AGENT_ID` (the bridge defaults to `default`
     when the variable is unset; check the compose file before creating the token).
   - Limit **Projects** to the repositories that bridge may work on; leave blank for all.
   - Enable the jobs it supports: `review` and (if it can open/merge PRs) `handoff`.
   - Copy the `rl_agt_…` value once.
2. Update the `redline-agent` container environment:
   ```yaml
   environment:
     REDLINE_URL: https://redline.ibolya.cz
     REDLINE_TOKEN: rl_agt_…        # the new agent token
     REDLINE_AGENT_ID: redline-agent
   ```
3. Restart only the bridge. The server starts accepting its scoped token immediately; revoking
   the token later affects no other agent or human session.

An agent token is rejected on human routes, so the bridge cannot list or edit reviews outside its
project allowlist. `/api/agent/next` and MCP `list_reviews`/`get_feedback`/`post_round` honor the
same scope. If the token's agent id and the bridge's `REDLINE_AGENT_ID` differ, `next` answers
`403` — fix the id rather than widening the token.

## 5. Extension, CLI and scripts

1. Open **Tokens → Personal tokens** (also linked from the notification bell).
2. Create one token per client:
   - extension → scope it to the projects you use, leave it writable (it submits screenshots),
   - CLI/scripts → scope and mark **read-only** unless they need to submit.
3. Replace the old server token in the browser extension options / CLI config with the
   `rl_pat_…` value. PATs inherit your project roles, so removing you from a project immediately
   narrows the token too.
4. Keep each token in a password manager; the UI shows only its prefix after creation.

## 6. Verify

```bash
curl -s https://redline.ibolya.cz/health
docker exec redline /redline --version          # should print 0.28.x
docker logs redline | tail -50                  # no migration errors
```

Then check in the UI:

- the owner signs in and sees the old reviews,
- a member sees only the projects assigned to them,
- a tester gets **Request agent review**, a developer gets **Send to agent**,
- the header bell shows notifications and the **Audit log** contains `owner.bootstrap`,
  `user.create`, `membership.update`, `auth.login` and `token.create`,
- the bridge is **connected** (header chip) and can pick up one test review,
- `/tokens` lists prefixes but never raw values.

## 7. What changed in the data folder

New, additive files (all safe to back up):

| File | Purpose |
|---|---|
| `users.json` | named accounts, Argon2id hashes (`0600`) |
| `server.json` | the single `owner_id` |
| `auth-sessions.json` | hashed browser sessions |
| `tokens.json` | hashed PATs and agent tokens |
| `invites.json` | hashed one-time invite/reset links |
| `memberships.json` | per-project roles |
| `audit.jsonl` | append-only audit (1 year default) |
| `notifications/<user>.jsonl` | per-user in-app notifications (90 days default) |
| `sessions/<id>/comments.jsonl` | human discussion for that task |

`meta.json` and `annotations.json` gain `revision`/`updated_by`/`updated_at`; old readers ignore
unknown fields. The session folder format itself is unchanged.

## 8. Rollback

The v0.27 image can start on the same data folder: it ignores the new files and the unknown
`revision` fields, and the legacy token remains valid. Stop the bridge first, redeploy the old
image, and point the bridge back to the old token if you changed it. Do not delete the new files;
they are the upgrade path for the next attempt. Restore the pre-upgrade backup if you need the
exact previous state.

## 9. Retention / housekeeping

- Audit: **365 days** (`REDLINE_AUDIT_RETENTION_DAYS` overrides).
- Notifications: **90 days** (`REDLINE_NOTIFICATION_RETENTION_DAYS` overrides).
- Invite/reset rows and unused links: 30 days after creation/expiry, and used links cannot be
  replayed.
- Old sessions keep their videos and history; session size is shown in History and is not
  pruned automatically.

## 10. Security notes

- The legacy token is still **root-equivalent** (owner + agent). After every human client uses a
  PAT and every bridge uses an agent token, rotate it: change `REDLINE_TOKEN` (or
  `/data/token`), restart the server, and update the migration fallback only if needed.
- Ownership transfer is owner-only, requires the current password and confirmation, and is
  audited. The old owner becomes a member immediately; project roles are preserved.
- No SMTP is used. Invite/reset links are the only onboarding path; treat them like passwords
  until used.
