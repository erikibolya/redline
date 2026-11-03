# Kolaborace více uživatelů — zadání

> **Stav:** schválené zadání k implementaci (rozhodnutí 2026-11-03). Cílový release: **v0.28**,
> samostatně s migračním guide.
>
> **Implementace:** auth foundation je v `dev` — uživatelé, Argon2id, serverové session/CSRF,
> login stránka, owner bootstrap a transfer. Navazuje projektová ACL: memberships, filtrování
> projektů/session/media, tester `Request agent review`, developer `Send to agent` a owner
> **People & access** matice. Diskuse a notifikace jsou také v `dev`: `comments.jsonl` v tasku,
> per-user `notifications/<user>.jsonl`, zvonek s unread badge, mentions a resolve; retence
> notifikací 90 dní (`REDLINE_NOTIFICATION_RETENTION_DAYS`). Audit je v `dev`: `audit.jsonl`,
> owner-only `/audit` s filtry, retence 1 rok (`REDLINE_AUDIT_RETENTION_DAYS`). Optimistická
> revize je v `dev`: `meta.revision` + `annotations.revision`, `base_revision` v submitu,
> strukturovaný `409` a UI banner s **Reload theirs / Keep mine**. PAT, invite/reset a MCP akce
> v auditu následují.
> **Cíl:** bezpečně pustit k jedné RedLine instanci více lidí, rozdělit je na role a omezit
> přístup k projektům, aniž by se rozbila současná cesta agent → review → člověk.
>
> **Rozhodnutý MVP:** lokální hesla + session cookie, **jeden Owner s předáním ownershipu**,
> Developer / Tester, projektová ACL, explicitní „ready for agent“, **komentáře, notifikace**,
> audit a optimistická revize. Bez SSO, bez e-mailového serveru a bez real-time CRDT.

## Shrnutí

- **Owner** je právě jeden globální vlastník serveru: uživatelé, projekty, členství, agent
  tokeny, audit. Ownership lze předat jinému uživateli; starý owner se stane běžným členem.
- **Developer** je člen projektu: vytváří tasky, edituje otevřené tasky, **odesílá agentovi**,
  stopuje run, potvrzuje/zrušuje feature a žádá handoff.
- **Tester** je člen projektu: **čte všechny tasky projektu**, ale edituje jen **vlastní**
  otevřené tasky. Tasky vytváří a anotuje, ale **nesmí je odeslat agentovi**; klikne
  **Request agent review** a developer je odešle.
- **Viewer** je volitelná read-only role; **agent** je servisní identita s allowlistem projektů.
- Přístup se nastavuje v owner-only **People & access** matici `uživatel × projekt`.
- Lidé se přihlašují **lokálním username/heslem** (session cookie); extension/CLI používají PAT;
  bridge používá **jeden z více service tokenů** s allowlistem. Stávající jediný token zůstane
  po migraci jako legacy owner/agent token s varováním.
- **Nový task vždy vyžaduje projekt**; Inbox je owner-only legacy prostor.
- **MVP = fáze 1+2+3**: auth + role/ACL + request + **komentáře/notifikace** + audit + revize.
  Presence, assignment, OIDC a share linky jsou další fáze.
- **Release:** samostatný **v0.28** s migračním guide a rollbackem.

### Rozhodnutí (2026-11-03)

| Otázka | Rozhodnutí |
|---|---|
| Přihlášení | **Lokální hesla** (OIDC/SSO mimo MVP) |
| Tester → agent | **Explicitní `Request agent review`** (stav „ready for agent“) |
| Tester scope | **Čte všechny tasky projektu, edituje jen vlastní** otevřené |
| Inbox | **Projekt povinný** pro nový task; Inbox jen owner |
| Owner | **Přesně jeden**, s možností **předat ownership** jinému uživateli |
| MVP | **Komentáře a notifikace rovnou** (spolu s auth, role/ACL, requestem a auditem) |
| Mazání tasků | Owner any; developer/tester jen **vlastní ne-submitted** |
| Správa členů | **Owner only** v MVP (project admin až později) |
| Invite/reset | **Jednorázové linky**, bez SMTP/e-mailu |
| Desktop local | **Owner-only bez loginu** |
| Extension | **PAT** (ne server token) |
| Pojmenování | **Task v UI**, **Review** pro agentní kolo; API názvy zůstávají |
| Agent tokeny | **Více tokenů** s vlastním allowlistem a joby |
| Share linky | **Až později** (mimo MVP) |
| Retence | Audit **1 rok**, notifikace **90 dní** |
| Release | **Samostatný v0.28** s migračním guide |

---

## 1. Co dnes máme

RedLine je dnes **jednouživatelský server s jedním tokenem**:

- `Auth` drží jeden token z `REDLINE_TOKEN` / `data/token`.
- Každé `/api` a `/mcp` volání projde middlewarem `require_token`; credential může být
  `Authorization: Bearer`, cookie `redline_token` nebo `?t=<token>`.
- `GET /s/<id>?t=<token>` vymění token za `HttpOnly; SameSite=Strict` cookie.
- `Sec-Fetch-Site: cross-site` je odmítnutý; výjimku má jen browser extension s platným tokenem.
- Desktop app ukládá do `config.json` profily `{url, token}`; extension ukládá `{serverUrl, token}`.
- `redline-agent` a MCP agenti používají stejný token jako člověk.
- Projekty jsou složky `sessions/projects/<slug>/`; `meta.project` rozhoduje, který repo bridge
  použije. Přístup k projektu dnes neexistuje — kdo má token, vidí všechno.
- `GET /api/sessions` vrací všechny session; `POST /api/sessions/{id}/annotations` je submit;
  `confirm-feature`, `cancel-feature`, `handoff`, `cancel` (STOP) nemají žádnou roli.
- `?t=` je root token v URL. To je dnes přijatelné jen proto, že existuje jediný uživatel.

**Důsledek:** „Tester nesmí odeslat task agentovi“ nejde vyřešit jen schováním tlačítka. Musí
existovat identita, projektová autorizace na serveru a nový stav, kterým tester předá task
developerovi.

---

## 2. Slovník

| Pojem | Význam |
|---|---|
| **Task** | Jedna session (`20261103-…`), screenshot/video + anotace + vlákno. Termín pro **lidské UI**. |
| **Review** | Agentovo kolo/výsledek na tasku; termín pro **API/MCP** (`review_screenshot`, `review.md`, `review round`). |
| **Project** | Konverzační a přístupová jednotka; dnes složka + `project.json`, volitelně `repo` pro agenta. |
| **Owner** | Vlastník celého serveru. Globální role, není vázaná na projekt. |
| **Developer** | Člen projektu s právem submitovat tasky agentovi, zastavit run, potvrdit/zrušit feature a žádat handoff. |
| **Tester** | Člen projektu, který tasky vytváří a anotuje, ale **nesmí je odeslat agentovi**; požádá developera. |
| **Viewer** | Volitelná read-only role pro stakeholdera. |
| **Agent** | Servisní identita bridge/MCP (není to člověk); vytváří review, čte feedback, postuje kola, reportuje handoff. |
| **Membership** | Dvojice `(user, project, role)`; uživatel může mít v různých projektech různé role. |
| **PAT** | Personal Access Token člověka pro CLI/extension/desktop; dědí jeho práva a lze ho dál omezit. |
| **Service/agent token** | Token pro `redline-agent`/MCP; není to uživatelský účet. |
| **Ready for agent** | Stav, kdy tester dokončil task a čeká, až ho developer odešle agentovi. |
| **Share link** | Volitelný, časově omezený odkaz na jeden task (read-only / annotate), bez přístupu k projektu. |

**Pojmenování (rozhodnuto):** v lidském UI používat **Task** — „New task“, „My tasks“, „Send to
agent“; pro testera „Request agent review“. Agentní/API vrstva zůstává **review** (`session`,
`review_screenshot`, `review.md`, `get_feedback`), aby se nerozbily klienti ani data. Iteraci
tasku říkáme **Round** (už dnes „Round N“ v History). UI tlačítko pro `POST /annotations` se
jmenuje **Send to agent** (dnes „Submit review“); endpoint se nemění. Důvod: „task“ je to, co
člověk zakládá a na čem tým pracuje; „review“ je to, co udělá agent. Není to kosmetika — mění to
texty tlačítek, filtry a dokumentaci, ale ne datové schéma.

---

## 3. Role a matice oprávnění

### 3.1 Definice

- **Owner** je **právě jeden** globální vlastník serveru. Vidí a může všechno, spravuje
  uživatele, projekty, členství, agent tokeny, serverové nastavení a audit. Ownership lze
  **předat** jinému aktivnímu uživateli; server nikdy nesmí mít nula ani dva ownery. Starý owner
  se po předání stane běžným `member` a zachová si své projektové členství, dokud ho nový owner
  nezmění.
- **Developer** je vždy člen konkrétního projektu. Může tasky vytvářet, editovat otevřené tasky
  projektu, odesílat je agentovi, zastavit běžící run, potvrdit/zrušit feature, žádat/retryovat
  handoff, komentovat a číst agentovo vlákno i výstup.
- **Tester** je vždy člen konkrétního projektu. **Čte všechny tasky projektu** a agentovo vlákno
  (read-only), ale **edituje pouze vlastní** otevřené tasky. Může tasky vytvářet, anotovat a
  komentovat, ale **nesmí** submitovat agentovi, zastavit run, potvrdit/zrušit feature ani žádat
  handoff. Místo submitnutí použije **Request agent review** (viz § 6.3).
- **Viewer** (volitelný) vidí tasky projektu a vlákno, může komentovat; needituje anotace ani
  nespouští agenta.
- **Agent** není uživatel v UI. Má service token s volitelným allowlistem projektů a jobů
  (`review`, `handoff`). Jeho akce se v auditu a na tasku zobrazují jako „agent“.

Uživatel může být v projektu A Developer, v projektu B Tester a v projektu C bez přístupu.
Owner tuto matici obchází.

**Předání ownershipu:** owner-only akce `POST /api/admin/transfer-owner` s cílovým `user_id`.
Vyžaduje aktivního uživatele s heslem, potvrzení a re-autentizaci současného ownera. Server
atomicky přepíše `server.json.owner_id`, změní starého ownera na `member` a zapíše audit
`owner.transfer`. Cílový uživatel dostane notifikaci. Starý owner si zachová dosavadní projektové
role; nový owner je může změnit. Pokud je cílový uživatel tester, předáním se z něj stane owner
(globální práva); jeho projektové role zůstávají, ale jsou irelevantní. Výchozí stav je vždy
právě jeden owner — žádné „dva owneři“ ani dočasné období bez ownera.

### 3.2 Matice akcí

`✅` = povoleno, `—` = zakázáno, `read` = jen čtení, `own` = jen vlastní/otevřené tasky.

| Akce | Owner | Developer | Tester | Viewer | Agent |
|---|---|---|---|---|---|
| Vidět projekt v railu / pickeru | ✅ all | ✅ member | ✅ member | ✅ member | allowlist |
| Číst task, anotace, historii, media | ✅ | ✅ | ✅ | ✅ | allowlist |
| Vytvořit task | ✅ | ✅ | ✅ | — | ✅ přes MCP |
| Editovat anotace otevřeného tasku | ✅ | ✅ | `own` | — | — |
| Smazat task | ✅ any | `own` ne-submitted | `own` ne-submitted | — | — |
| **Submit task agentovi** | ✅ | ✅ | **—** | — | — |
| **Request agent review** (tester → developer) | ✅ | ✅ | ✅ | — | — |
| Zastavit běžící agent run (STOP) | ✅ | ✅ | **—** | — | — |
| Potvrdit / zrušit feature | ✅ | ✅ | **—** | — | — |
| Žádat / retryovat handoff | ✅ | ✅ | **—** | — | — |
| Reportovat handoff výsledek | — | — | — | — | ✅ |
| Přesunout task mezi projekty | ✅ any | ✅ jen mezi svými projekty | — | — | — |
| Spravovat členy projektu | ✅ | — | — | — | — |
| Vytvořit / archivovat projekt | ✅ | — | — | — | — |
| Spravovat uživatele a role | ✅ | — | — | — | — |
| Spravovat agent tokeny | ✅ | — | — | — | — |
| Vidět audit | ✅ all | project-scoped (volitelně) | — | — | — |
| Komentovat task | ✅ | ✅ | ✅ | ✅ | — |
| Spravovat komentáře (resolve/smazat) | ✅ any | ✅ any | ✅ own | ✅ own | — |
| Vytvořit PAT | ✅ | ✅ | ✅ | ✅ | — |
| Vytvořit share link | ✅ | ✅ | — | — | — |
| Předat ownership | ✅ | — | — | — | — |

**Poznámky k matici:**

- „Own“ u testerských tasků znamená `meta.created_by == user.id`. Developer může editovat i task,
  který vytvořil tester, ale jen dokud není submitted; tím se dá testerův request dokončit.
- **Mazání (rozhodnuto):** owner může any; developer/tester jen **vlastní ne-submitted**.
  Submitted/resolved tasky se nemažou (jen owner, a i to s auditem a potvrzením). Tím se
  zachová historie.
- **Správa členů (rozhodnuto):** owner only. Project admin je vědomě mimo MVP; může přijít
  později, pokud bude potřeba delegovat.
- STOP (`POST /cancel`) je zásah do běhu; tester ho nesmí. Pokud tester potřebuje task zastavit,
  požádá developera.
- Bulk akce (confirm/cancel/delete) musí na serveru ověřit každý task zvlášť; UI nesmí poslat
  task, na který uživatel nemá právo.
- Agent má vlastní allowlist; projektová ACL člověka ho neblokuje. Agent ale nesmí vidět projekt,
  který nemá v allowlistu.

---

## 4. Projektový přístup (ACL)

### 4.1 Model

- `Project` zůstává složkou `sessions/projects/<slug>/` s `project.json` (name, repo, agent,
  max_parallel).
- `Membership` je samostatný záznam `(project_slug, user_id, role)`, kde role je
  `developer` / `tester` / `viewer`. Owner membership se neukládá — owner je globální.
- Uživatel může mít v každém projektu jinou roli. Jedna dvojice `(project, user)` má nejvýš
  jednu roli.
- Projekt vzniká dvěma cestami:
  1. Owner ho vytvoří ručně (label-only nebo s repo klíčem).
  2. Agent ohlásí repo přes `POST /api/agent/repos`; projekt vznikne automaticky. Owner dostane
     upozornění „nový repo projekt — přiřaď členy“.
- Dokud repo projekt nemá žádného developera, agent do něj nesmí být poslán (submit vrátí 409
  s jasnou zprávou). Tester může task vytvořit, ale zůstane ve frontě „ready for agent“.
- **Inbox (rozhodnuto):** nový task **musí mít projekt**, do kterého má uživatel právo. Inbox
  zůstává jen jako legacy/owner prostor; běžný uživatel ho v pickeru neuvidí. Důvod: Inbox nemá
  repo a bridge dnes tiše padá na default repo; s více uživateli by to byl bezpečnostní a
  routovací problém. Owner může Inbox použít pro soukromé review; agent do Inboxu nesmí, dokud
  owner výslovně nenastaví default repo.
- **Default projekt** pro nový task: poslední projekt, ve kterém uživatel pracoval (dnes
  `lastProject` v localStorage). S multi-user to může zůstat localStorage, případně per-user
  preference na serveru (volitelné).
- **Přístup k projektům v UI:** `GET /api/projects` vrací jen projekty, kde je uživatel členem
  (owner všechny). `GET /api/sessions` vrací jen tasky z těchto projektů. `GET /api/sessions/{id}`
  a všechny media/history/trace routes vrací **404** (ne 403) pro cizí task, aby nešlo enumerovat
  existence.
- **Změna členství** platí okamžitě: každý request autorizuje znovu. Otevřený browser dostane při
  dalším pollu 401/404 a UI ho přesměruje na login/„nemáš přístup“.
- **Přesun tasku mezi projekty** (funkce z v0.27) musí respektovat ACL:
  - owner může kamkoli,
  - developer jen pokud je členem zdrojového i cílového projektu a má v obou developer roli,
  - tester nikdy,
  - cílový projekt musí mít repo, jinak task nelze odeslat agentovi.

### 4.2 UI pro správu přístupu

Owner-only stránka **People & access** (může být `/admin/people` nebo dialog z user menu):

```
┌──────────────────────────────────────────────────────────────────────┐
│ People & access                                   [Invite user]      │
├──────────────────────────────────────────────────────────────────────┤
│ User              Inbox   Au.Net       RedLine      DANEEK           │
│ Anna (owner)      owner   owner        owner        owner             │
│ Petr Novák        —       Developer    Tester       —                 │
│ Jana Dvořáková    —       Tester       Tester       Developer         │
│ Agent: holly      —       allowlist    allowlist    —                 │
└──────────────────────────────────────────────────────────────────────┘
```

- Buňka je select: `—` / `Viewer` / `Tester` / `Developer`. Owner řádek je read-only.
- Klik na uživatele otevře detail: jméno, e-mail, stav, poslední login, členství, PATy, sessions,
  „Reset password“, „Disable“, „Revoke sessions“.
- Klik na projekt otevře **Project members**: seznam členů s rolemi, repo, agent, varování
  „nemá developera“.
- Invite dialog: username, display name, e-mail (volitelně), project roles; vytvoří se jednorázový
  invite link (7 dní) nebo dočasné heslo; uživatel si při prvním loginu změní heslo.
- Owner může **předat ownership** jinému aktivnímu uživateli; server vždy musí mít právě
  jednoho ownera. Owner nemůže smazat sám sebe ani zůstat bez ownera.
- Owner může vidět „unassigned repo projects“ a rychle přiřadit developery.

---

## 5. Uživatelské scénáře

### 5.1 Owner poprvé spustí multi-user verzi

1. Server startuje s existujícím `data/token` a bez `users.json`.
2. Owner otevře `/setup` (nebo CLI) a vytvoří owner účet: username, display name, heslo.
3. Stávající token se uloží jako **legacy owner PAT** a jako první **agent token** (`bridge`).
   UI ho zobrazí se zřetelným varováním „tento token má plná práva; použij ho pro agenta a pak
   ho rotuj“.
4. Všechny existující tasky se přiřadí `created_by = owner` a všechny projekty jsou ownerovy.
5. Owner pozve ostatní uživatele a přiřadí jim projekty.

### 5.2 Tester vytvoří task a požádá o agenta

1. Tester se přihlásí, v pickeru vidí jen své projekty.
2. Vytvoří task (screenshot/video), anotuje, napíše prompt.
3. Místo **Send to agent** vidí **Request agent review**. Klik uloží jeho práci jako „ready“
   (viz § 6.3) a upozorní developery projektu.
4. Tester může request zrušit nebo upravit, dokud ho developer neodešle.
5. Developer dostane v railu badge **ready for agent**, otevře task, zkontroluje/upraví anotace
   a klikne **Send to agent**.
6. Agent pracuje; tester i developer vidí průběh a odpověď. Tester **nesmí** STOP, confirm,
   cancel ani handoff.
7. Developer po dokončení potvrdí feature (a případně handoff).

### 5.3 Developer vytvoří a odešle task

Stejné jako dnes, jen s kontrolou projektu a role. Developer vidí všechny tasky svého projektu,
může editovat otevřené tasky, submitovat, stopovat a rozhodovat.

### 5.4 Owner odebere přístup

1. Owner nastaví uživateli v projektu `—` nebo ho disable.
2. Server okamžitě odmítne jeho requesty (401/404) a zruší jeho auth sessions.
3. Tasky zůstávají; `created_by` se zobrazí jako „Jana (odebrán)“.
4. Audit zapíše `membership.remove` / `user.disable` s aktérem a časem.

### 5.5 Agent vytvoří review přes MCP

1. Agent použije service token s allowlistem projektu `Au.Net`.
2. `review_screenshot` vytvoří task s `created_by = agent:holly`, `project = actiunit-au-net`.
3. Task vidí všichni členové projektu; submit do agenta může udělat developer/owner.
4. Agent vidí jen tasky v allowlistu; `list_reviews` je filtruje.

### 5.6 Externí tester / klient

Volitelně owner nebo developer vytvoří **share link** na jeden task (read-only nebo annotate,
expirace 7 dní). Odkaz neumožní seznam projektů ani jiné tasky. Vhodné pro klienta, který nemá
účet. Share link se dá kdykoli revokovat.

### 5.7 Owner předá ownership

1. Owner otevře **People & access → Transfer ownership**, vybere aktivního uživatele.
2. Potvrdí akci svým heslem (re-auth) a dostane vysvětlení: starý owner se stane `member`,
   cílový user získá globální práva.
3. Server atomicky přepne `server.json.owner_id`, zapíše audit `owner.transfer` a pošle
   notifikaci cílovému uživateli.
4. Starý owner si zachová své projektové role; nový owner je může upravit.
5. Pokud akce selže, owner zůstává beze změny (žádný částečný stav).

---

## 6. Účet, přihlášení a tokeny

### 6.1 Dva druhy credentialů

1. **Human session** — přihlášení uživatele, serverová session, `HttpOnly` cookie.
2. **Token** — PAT člověka nebo service token agenta, posílaný v `Authorization: Bearer`.
   Nikdy se neposílá v URL (výjimka: jednorázový login/invite token, viz níže).

**MVP používá lokální username + heslo.** OIDC/SSO je volitelná další fáze; nic v modelu na něm
není závislé. Samoregistrace neexistuje — uživatele zakládá owner pozvánkou nebo dočasným heslem.

`?t=` s root tokenem se po migraci nesmí používat pro běžné lidské odkazy. Nahradí ho:
- login přes `/login`,
- jednorázový login token (`/login?token=…`, TTL 60 s) pro desktop/CLI,
- volitelný share link na konkrétní task.

### 6.2 Login a session

- `POST /api/auth/login {username, password}` → nastaví `redline_session` cookie a vrátí
  `{user, memberships, csrf_token}`.
- Cookie: `HttpOnly; Secure` (v produkci); `SameSite=Strict`; `Path=/`; `Max-Age` 30 dní;
  session ID je 32 random bytes, na serveru jen hash.
- `POST /api/auth/logout` zruší session; `GET /api/auth/me` vrátí identitu a práva.
- `GET /api/auth/sessions` / `DELETE /api/auth/sessions/{id}` — zařízení; „revoke all“.
- `POST /api/auth/password` — změna hesla; revokuje ostatní sessions.
- **Invite/reset (rozhodnuto):** owner vygeneruje jednorázový odkaz (invite 7 dní, reset 24 h)
  a zkopíruje ho; e-mail/SMTP se v MVP nepoužívá. Odkaz je hashovaný, jednorázový a auditovaný.
- Login rate limit: 10 pokusů/min na IP+username, exponential backoff, audit; po 5 chybách
  dočasný lock. Session fixation: po loginu vždy nová session ID.
- CSRF: SameSite=Strict + `Sec-Fetch-Site` guard + CSRF token pro state-changing requesty
  (`X-CSRF-Token`, hodnota z `GET /api/auth/me` nebo z non-HttpOnly `redline_csrf` cookie).
- `Referrer-Policy: no-referrer` a `Cache-Control: no-store` na stránkách s tokeny.

### 6.3 Tester „Request agent review“ — stav a data

**Rozhodnutý model:** request **uloží testerskou práci** a označí task jako připravený, ale
nezmění ho na `submitted` (agent ho nesmí vzít).

- `POST /api/sessions/{id}/request-agent` (tester/developer/owner):
  - uloží aktuální anotace, PNG, `review.md` a prompt do `pending/` ve stejném formátu jako
    submit,
  - nastaví `agent_requested_at`, `agent_requested_by`, `agent_request_note` (volitelně),
  - `status` zůstane `open`; agent `next` ho ignoruje,
  - pokud task už má `agent_requested_at`, jde o update requestu.
- Developer otevře task, uvidí banner „Requested by Jana · ready for agent“, může anotace
  upravit a klikne **Send to agent**.
- Submit musí ověřit, že uživatel má v projektu `developer` (nebo owner) a že projekt má repo.
  Teprve submit nastaví `submitted_at`, `status=submitted` a vytvoří round pro agenta.
- Historie: request se nemá objevit jako samostatné kolo. Buď:
  - **A)** request uloží jen `pending/` soubory a submit z nich udělá první kolo, nebo
  - **B)** request zapíše normální `annotations.json` a submit při prvním odeslání nearchivuje
    (protože aktuální round ještě nebyl agentem přijat).
  **Volím A** (`pending/`), protože tester může requestovat opakovaně a developer vidí poslední
  verzi; současně zůstane `annotations.json` jako poslední agentem viděné kolo.
- Tester může request zrušit (`DELETE /request-agent`) a task dál editovat.
- Tester nemůže submitovat ani kdyby obešel UI: `POST /annotations` s jeho session vrátí 403.
- Developer může task převzít i bez requestu (otevřený task v jeho projektu), ale badge
  „ready for agent“ je signál, že na tasku někdo čeká.

### 6.4 PAT (Personal Access Token)

- `GET/POST /api/me/tokens`, `DELETE /api/me/tokens/{id}`.
- Token se zobrazí **jednou**; na serveru jen hash + prefix pro UI.
- Dědí práva uživatele; volitelně `project_allowlist` a `read_only`.
- Expirace: 90 dní default, max 1 rok; „no expiry“ jen owner.
- `last_used_at`, `created_at`, `name`, `revoked_at`.
- Použití: browser extension, CLI, skripty, případně desktop.
- PAT nesmí eskalovat: pokud uživatel ztratí projekt, PAT tam ztratí přístup.

### 6.5 Service/agent token

- Owner spravuje **seznam** tokenů přes `GET/POST /api/admin/agent-tokens`.
- Každý token má `name`, `project_allowlist`, `jobs` (`review`, `handoff`), `last_used_at`,
  `revoked_at`.
- **Více tokenů (rozhodnuto):** každý bridge/agent může mít vlastní token s vlastním allowlistem;
  rotace jednoho neovlivní ostatní. `redline-agent` používá jeden ze svých tokenů, nikdy owner
  session.
- Token může být scoped na jeden projekt nebo na víc; prázdný allowlist znamená žádný projekt,
  ne všechny.
- Agent token nesmí spravovat uživatele ani projekty.

### 6.6 Owner bootstrap a recovery

- Nová instalace: `/setup` vytvoří ownera; token z instalace se uloží jako **první agent token**.
- Existující instalace: owner se vytvoří z `REDLINE_TOKEN` + nového hesla; token zůstane jako
  legacy owner PAT a první agent token.
- Recovery: lokální CLI `redline owner reset --data-dir …` (vyžaduje filesystem) nastaví nové
  heslo, revokuje sessions a vypíše jednorázový login token. Bez filesystemu není recovery
  (vlastnost self-hosted).
- **Právě jeden owner**; owner nesmí smazat sám sebe. Změna je jen přes transfer ownershipu.

### 6.7 Volitelně OIDC/SSO (mimo MVP)

Server je za Traefikem; pokud už běží Authelia/Cloudflare Access, dává smysl OIDC v pozdější
fázi. MVP je čistě lokální heslo:

- `REDLINE_OIDC_ISSUER`, `CLIENT_ID`, `CLIENT_SECRET`, `REDIRECT_URL`.
- Mapování claimů: `sub` → user, `email`/`name`; groups → role/projects (konfigurovatelné).
- Lokální owner účet zůstává jako break-glass.
- Pokud OIDC není, lokální hesla + PAT jsou plnohodnotná varianta.

---

## 7. Datový model

Vše je souborové, bez DB (zachovat D2). Nové soubory vedle `sessions/`:

```
data/
  token                      # legacy / bootstrap (migrace)
  server.json                # owner_id + schema version
  users.json                 # nebo users/<id>.json
  memberships.json           # (project, user, role)
  auth-sessions.json         # hashované session ID
  personal-tokens.json       # hashované PAT
  agent-tokens.json          # hashované service tokeny
  notifications/<user>.jsonl # per-user notifikace (MVP)
  users/<id>/preferences.json
  sessions/<id>/comments.jsonl  # lidská diskuse (MVP)
  sessions/…                 # beze změny
```

### 7.1 `users.json` (nebo `users/<id>.json`)

```json
{
  "id": "usr_01J…",
  "username": "jana",
  "display_name": "Jana Dvořáková",
  "email": "jana@example.com",
  "password_hash": "$argon2id$…",
  "role": "owner | member",
  "status": "invited | active | disabled",
  "created_at": "2026-11-03T18:00:00+00:00",
  "last_login_at": "2026-11-03T18:05:00+00:00",
  "must_change_password": false
}
```

`role: owner` je globální; běžný uživatel má `member` a projektové role v membershipech.
**Právě jeden** uživatel má `role: owner`; invariant hlídá `server.json`.

### 7.1a `server.json`

```json
{
  "schema": 2,
  "owner_id": "usr_01J…"
}
```

`owner_id` je jediný zdroj pravdy o ownerovi. Transfer ownershipu přepíše tento soubor
atomicky a teprve pak upraví `users.json` (starý owner → `member`). Start serveru odmítne
chybějícího/neexistujícího ownera a nabídne recovery.

### 7.2 `memberships.json`

```json
{
  "memberships": [
    { "project": "actiunit-au-net", "user": "usr_01J…", "role": "developer" },
    { "project": "redline", "user": "usr_01J…", "role": "tester" }
  ]
}
```

Unikátní `(project, user)`. Role: `developer | tester | viewer`.

### 7.3 `auth-sessions.json`

```json
{
  "sessions": [
    {
      "id_hash": "sha256:…",
      "user": "usr_01J…",
      "created_at": "…",
      "last_seen_at": "…",
      "expires_at": "…",
      "ip": "…",
      "user_agent": "…",
      "revoked_at": null
    }
  ]
}
```

### 7.4 `personal-tokens.json` / `agent-tokens.json`

```json
{
  "tokens": [
    {
      "id": "pat_01J…",
      "user": "usr_01J…",
      "name": "Extension – Chrome",
      "prefix": "rl_pat_7f3a",
      "hash": "sha256:…",
      "project_allowlist": ["actiunit-au-net"],
      "read_only": false,
      "created_at": "…",
      "last_used_at": "…",
      "expires_at": "…",
      "revoked_at": null
    }
  ]
}
```

Agent token má místo `user` pole `agent` a `jobs`.

### 7.5 Audit

`audit.jsonl`, jeden řádek = jedna akce:

```json
{"at":"2026-11-03T18:10:00+00:00","actor_type":"user","actor_id":"usr_01J…","actor_name":"Jana","action":"review.request_agent","project":"actiunit-au-net","session":"20261103-…","target":null,"ip":"…","user_agent":"…","details":{}}
```

Akce minimálně: `auth.login`, `auth.logout`, `auth.failed`, `user.invite`, `user.disable`,
`user.delete`, `membership.add/update/remove`, `project.create/archive`, `review.create`,
`review.request_agent`, `review.submit`, `review.stop`, `review.confirm`,
`review.cancel_feature`, `review.handoff`, `review.move_project`, `review.delete`,
`comment.create/delete/resolve`, `notification.read`, `owner.transfer`, `token.create/revoke`,
`share.create/revoke`, `settings.update`.

### 7.6 Rozšíření `meta.json`

```json
{
  "created_by": "usr_01J…",
  "created_by_name": "Jana Dvořáková",
  "created_by_type": "user | agent",
  "updated_by": "usr_01J…",
  "revision": 7,
  "agent_requested_at": "…",
  "agent_requested_by": "usr_01J…",
  "assigned_to": "usr_01J…",
  "feature_confirmed_by": "usr_01J…",
  "feature_confirmed_by_name": "Petr",
  "agent_token": "agent_holly"
}
```

`revision` je monotónní číslo pro optimistickou konkurrenci. `annotations.json` nese stejné
`revision`; submit posílá `base_revision` a server odmítne 409, pokud se změnilo.

### 7.7 Rozšíření `project.json`

```json
{
  "slug": "actiunit-au-net",
  "name": "Au.Net",
  "repo": "ActiUnit/Au.Net",
  "agent": "holly",
  "archived": false,
  "created_by": "usr_01J…",
  "created_at": "…"
}
```

Členové jsou v `memberships.json`, ne v `project.json` (jeden zdroj pravdy).

### 7.8 Komentáře a notifikace (MVP)

**`sessions/<id>/comments.jsonl`** — lidská diskuse, oddělená od agentova vlákna:

```json
{"id":"cmt_01J…","author":"usr_01J…","author_name":"Jana","at":"2026-11-03T18:20:00+00:00","body":"Prosím zkontrolovat hover stav.","mentions":["usr_02K…"],"resolved_at":null}
```

- `author` může editovat/smazat vlastní komentář; developer/owner může resolve/smazat any.
- `mentions` jsou user id; server ověří, že jsou členy projektu, a pošle notifikaci.
- Komentáře nesmí obsahovat tokeny/hesla; max. délka a rate limit.

**`notifications/<user>.jsonl`** — per-user inbox:

```json
{"id":"ntf_01J…","at":"2026-11-03T18:21:00+00:00","kind":"review.ready","project":"actiunit-au-net","session":"20261103-…","actor":"usr_01J…","actor_name":"Jana","text":"Jana požádala o agent review","url":"/s/20261103-…","read_at":null}
```

- Události: `review.created`, `review.ready`, `review.submitted`, `review.replied`,
  `review.resolved`, `review.handoff_done`, `review.handoff_failed`, `comment.mention`,
  `review.assigned`, `access.changed`, `owner.transfer`.
- Notifikace jsou jen pro členy projektu; nikdy nesmí prozradit cizí projekt/task.
- `read_at` + „mark all read“; retence 90 dní (konfigurovatelné).
- MVP je jen in-app (zvonek); e-mail/Slack je další fáze.

---

## 8. API

### 8.1 Auth kontext a middleware

- `require_auth` rozliší:
  - `Authorization: Bearer rl_pat_…` / `rl_agent_…` (hash lookup),
  - `redline_session` cookie (hash lookup),
  - legacy `redline_token` (migrace; mapuje se na ownera, dokud owner token nerevokuje),
  - `?t=` jen pro jednorázový login token, jinak 401.
- Do requestu se vloží:

```rust
struct AuthContext {
    actor: Actor,          // User { id, name, is_owner } | Agent { id, name, projects } | Share { session, mode }
    projects: Map<String, Role>, // efektivní role; owner = all
    csrf: bool,
}
```

- Helpery: `require_owner`, `require_project(project, min_role)`, `require_review(id, action)`.
- `require_review` načte `meta.project` a ověří členství; cizí/neexistující → **404**.
- Všechny routes s `{id}` (image, media, history, trace, annotated, review.md, rounds) musí
  použít stejnou kontrolu. Dnes je chrání jen globální token.

### 8.2 Nové endpointy

| Metoda | Cesta | Kdo | Účel |
|---|---|---|---|
| `POST` | `/api/auth/login` | public | login, nastaví session cookie |
| `POST` | `/api/auth/logout` | user | zruší session |
| `GET` | `/api/auth/me` | user | identita, role, projekty, CSRF |
| `POST` | `/api/auth/password` | user | změna hesla |
| `GET/DELETE` | `/api/auth/sessions[/{id}]` | user | zařízení, revoke |
| `GET/POST` | `/api/me/tokens` | user | PAT |
| `DELETE` | `/api/me/tokens/{id}` | user | revoke PAT |
| `GET/POST` | `/api/users` | owner | seznam / invite |
| `GET/PATCH/DELETE` | `/api/users/{id}` | owner | detail, disable, delete |
| `POST` | `/api/users/{id}/reset` | owner | reset hesla / invite link |
| `GET/PUT` | `/api/projects/{slug}/members` | owner | členové projektu |
| `PUT` | `/api/memberships` | owner | matice / bulk update |
| `GET/POST` | `/api/admin/agent-tokens` | owner | service tokeny |
| `DELETE` | `/api/admin/agent-tokens/{id}` | owner | revoke |
| `GET` | `/api/audit` | owner | audit, filtry |
| `GET` | `/api/projects/{slug}/audit` | owner / developer | project audit (volitelně) |
| `POST` | `/api/admin/transfer-owner` | owner | předání ownershipu (heslo + target) |
| `POST` | `/api/sessions/{id}/request-agent` | tester+ | uloží request, upozorní developery |
| `DELETE` | `/api/sessions/{id}/request-agent` | tester+ | zruší request |
| `POST` | `/api/sessions/{id}/share` | developer+ | share link |
| `DELETE` | `/api/shares/{id}` | developer+ | revoke |
| `GET` | `/api/notifications` | user | notifikace, unread count |
| `POST` | `/api/notifications/read` | user | označit přečtené |
| `GET/POST` | `/api/sessions/{id}/comments` | member | lidská diskuse |
| `PATCH/DELETE` | `/api/sessions/{id}/comments/{comment}` | author / developer+ | edit/resolve/delete |
| `POST` | `/api/sessions/{id}/presence` | member | heartbeat „viewing/editing“ (volitelně) |

### 8.3 Změněné endpointy

- `GET /api/projects` — jen projekty, kde je uživatel členem; owner všechny; každý projekt nese
  `my_role` a `member_count`.
- `GET /api/sessions` — jen tasky z dostupných projektů; přidat `my_role`, `created_by_name`,
  `agent_requested_at`, `assigned_to`, `revision`.
- `GET /api/sessions/{id}` — 404 pro cizí; přidat `my_role`, `created_by`, `revision`.
- `POST /api/sessions` — `project` povinný pro každý nový task; ověřit `review.create`.
  Výjimku má jen owner, který může založit legacy task v Inboxu.
- `POST /api/sessions/{id}/annotations` — jen developer/owner; ověřit `review.submit`; 403 pro
  testera; `base_revision` pro konflikt.
- `POST /api/sessions/{id}/cancel` (STOP) — jen developer/owner.
- `POST /api/sessions/{id}/resolve` — jen agent token (dnes ho volá agent); `reopen` — jen
  developer/owner.
- `POST /api/sessions/{id}/confirm-feature`, `cancel-feature`, `handoff`, `/reopen` — jen
  developer/owner.
- `PUT /api/sessions/{id}/branch` — jen developer/owner.
- `PUT /api/sessions/{id}/project` — owner, nebo developer se členstvím ve zdroji i cíli.
- `DELETE /api/sessions/{id}` — owner, nebo vlastník ne-submitted tasku; 403/409 jinak.
- Všechny media/history/trace routes — `require_review(read)`.
- `POST /api/agent/*` a `/mcp` — jen agent token; allowlist projektů; `created_by=agent`.
- `GET /api/agent/next` — jen tasky v allowlistu; jinak stejné.
- `POST /mcp` `list_reviews` — filtrovat allowlistem.

### 8.4 Chybové kódy

- `401` — chybí/neplatná session/token; UI přesměruje na login.
- `403` — přihlášený, ale akce není povolena (např. tester submit); UI ukáže jasnou zprávu.
- `404` — cizí/neexistující projekt/task (proti enumeraci).
- `409` — konflikt revizí, submit bez developera, handoff běží, projekt bez repo.
- `422` — validační chyba (např. neplatná role).

---

## 9. UI

### 9.1 Login a účet

- `/login`: username + password, „Remember me“, volitelně „Sign in with token“ pro migraci.
- Po loginu redirect na původní URL (bez tokenu v URL).
- User menu v headeru: jméno, role v aktuálním projektu, „Account“, „People & access“ (owner),
  „Log out“.
- `/account`: profil, změna hesla, aktivní sessions, PATy (vytvořit/revokovat), poslední login.
- Pokud user nemá žádný projekt: prázdný stav „Požádej ownera o přístup k projektu“.

### 9.2 Editor

- Header: project chip (už máme), branch chip, user menu.
- Nový task: project picker jen s dostupnými projekty; pro testera submit button
  **Request agent review**, pro developera **Send to agent**.
- Tester vidí banner „Tento task odešle agentovi developer“ a po requestu „Waiting for a
  developer“.
- Developer vidí u requestu badge **ready for agent** a tlačítko **Send to agent**.
- Pokud je task cizí a uživatel je tester, editor je read-only (anotace i submit composer),
  ale může číst vlákno a **komentovat**.
- STOP, confirm, cancel, handoff jsou pro testera skryté/disabled s tooltipem „Jen developer“.
- **Discussion** tab v inspectoru: lidské komentáře, @mentions, resolve; oddělené od agentova
  vlákna. Tester může komentovat i cizí task ve svém projektu.
- Při `revision` konfliktu: banner „Někdo jiný upravil tento task“ + `Reload` / `Keep mine`
  (keep mine jen developer s právem; server vrátí 409 a novou revizi).
- Presence (volitelně): „Jana edituje“ u tasku; heartbeat 30 s.

### 9.3 Rail a dashboard

- Filtry: `All`, `Active`, `Done` + nové `My tasks`, `Ready for agent` (developer).
- Projektové skupiny jen s dostupnými projekty; count respektuje ACL.
- Testerův task má badge `ready for agent` (amber) nebo `waiting for developer`.
- Owner vidí všechny projekty; cizí projekty v railu nemá.
- Notifikace: zvonek s unread count; události „nový task v projektu“, „ready for agent“,
  „agent odpověděl“, „handoff done/failed“, „mention“.
- Bulk akce: jen tasky, na které má uživatel právo; server validuje každý.

### 9.4 Admin

- **People & access** (owner): matice users × projects; invite; user detail; project members;
  **Transfer ownership** (heslo + cílový uživatel + potvrzení).
- **Agent tokens** (owner): seznam, vytvořit, revokovat, last used, allowlist.
- **Audit** (owner): filtr actor/action/project/date; stránkování.
- **Server** (owner): default repo, agent settings, session TTL, invite TTL, retention.
- **Project detail** (owner): repo, agent, members, „archive project“.
- **Comments/notifications** jsou součástí editoru a railu, ne admin stránky.
- Empty states: „Zatím žádní uživatelé — pozvi první“, „Tento repo projekt nemá developera“.

### 9.5 Wireframe (orientační)

```
Login                         People & access
┌─────────────────────┐       ┌────────────────────────────────────────────┐
│ RedLine             │       │ Users            Inbox  Au.Net  RedLine    │
│ Username [________] │       │ Anna (owner)     owner  owner   owner      │
│ Password [________] │       │ Petr             —      Dev     Tester     │
│ [ Sign in ]         │       │ Jana             —      Tester  Tester     │
│ Sign in with token  │       │ [+ Invite user]                            │
└─────────────────────┘       └────────────────────────────────────────────┘
```

---

## 10. Agent bridge a MCP

- Bridge používá **jeden ze service tokenů**, nikdy owner token. Každý token má vlastní allowlist
  projektů a jobs; více agentů/bridge může mít vlastní token.
- `GET /api/agent/next` vrací jen tasky v allowlistu toho kterého tokenu; claim/lease/heartbeat
  beze změny.
- `review_screenshot`/`review_video`/`capture_review` vytvoří task s `created_by=agent`;
  projekt musí být v allowlistu a mít repo (nebo explicitní default repo ownera).
- `get_feedback`, `post_round`, `progress`, `reply`, `resolve`, `handoff_status` beze změny,
  ale s allowlistem a auditem.
- Agent nikdy nevidí uživatelské PATy ani hesla.
- Bridge token se rotuje nezávisle na lidech; pokud owner token zneplatní, bridge jede dál.
- `handoff`: only developer/owner may confirm/request; the agent reports; the task records
  `feature_confirmed_by` a `handoff.requested_by`.
- `list_reviews` a `list_projects` filtrují allowlist; agent nesmí enumerovat cizí projekt.
- Pokud projekt nemá repo, submit vrátí 409 „project has no repository“; UI to řekne předem.
- Bridge dál nesmí tiše spadnout na default repo pro Inbox; s multi-user je Inbox owner-only.

---

## 11. Desktop, extension, CLI

### 11.1 Desktop app

- **Local (This computer) — rozhodnuto:** zůstává single-user owner; lokální server nevyžaduje
  login (nebo automaticky ownera). Vhodné pro sólo práci.
- **Remote profil**: místo tokenu v `config.json` otevře login stránku serveru a uloží session
  cookie ve webview; profil může mít volitelně PAT pro CLI/automatizaci.
- Setup wizard: URL → login → výběr projektu; „paste token“ zůstává jako fallback pro migraci.
- `Servers` stránka: jméno, URL, logged-in user, role, logout; přepnutí profilu neztratí session
  ostatních (cookies jsou per origin).
- `Back to sessions` musí respektovat aktivní profil (už dnes).
- Lokální MCP config používá **vlastní agent token** s allowlistem, ne owner token.

### 11.2 Browser extension

- Options (rozhodnuto): Server URL + **PAT** (ne server token) + projekt (filtrovaný podle
  PAT) + branch.
- PAT se ukládá jen v extension storage; nikdy se neloguje.
- Tester: extension umí vytvořit task a anotovat; „Send to agent“ nahradí „Save as ready“ /
  „Request agent review“; developer dostane notifikaci.
- Developer: stejné jako dnes, může submitnout.
- Offline export zůstává.
- CORS/extension origin zůstává; PAT v `Authorization`, ne v URL.

### 11.3 CLI

- `redline open` používá agent token nebo PAT; `--token` bere PAT/service token.
- `redline token` už netiskne owner token bez varování; `redline token --agent` / `--pat`.
- `redline mcp-config` generuje config s agent tokenem.
- Nově `redline login` (uloží PAT/session) a `redline whoami` (role/projects).
- `redline owner reset` pro recovery (lokální filesystem).

---

## 12. Souběh a kolaborace

Permisse samy nestačí; více lidí bude editovat stejný task.

### 12.1 Optimistická konkurrence (povinné)

- `meta.revision` a `annotations.revision`; každá změna +1.
- Submit/update posílá `base_revision`; server porovná:
  - shoda → zapíše, vrátí novou revizi,
  - neshoda → `409 { current_revision, updated_by, updated_at }`.
- UI při 409 nabídne `Reload` (zahodí lokální změny) nebo `Keep mine` (jen developer; pošle
  novou revizi jako novou baseline). Nikdy tiše nepřepisovat.
- Drafty v localStorage klíčovat `redline:draft:<user_id>:<session_id>`; jiný uživatel ve
  stejném browseru nesmí vidět cizí draft.

### 12.2 Presence (volitelné, doporučené)

- `POST /api/sessions/{id}/presence {mode: viewing|editing}` každých 20–30 s.
- Server drží TTL 60 s; UI ukáže avatary/„Jana edituje“.
- Není to hard lock; jen signál. Hard lock by blokoval práci při pádu prohlížeče.

### 12.3 Lidská diskuse (MVP)

- `comments.jsonl` per task: `{id, author, at, body, mentions, resolved}`.
- Samostatný tab „Discussion“ v inspectoru; nemíchat s `agent-thread.jsonl`.
- @mention → notifikace; „resolved“ vlákna.
- Tester může komentovat i cizí task; developer/owner může označit resolved.
- Autor může editovat/smazat vlastní komentář; developer/owner any.

### 12.4 Notifikace (MVP)

- Per-user `notifications/<user>.jsonl` s `read_at`.
- Události: nový task v projektu, ready for agent, agent reply/resolve, handoff done/failed,
  mention, assignment, access change, owner transfer.
- UI: zvonek + „Mark all read“; in-app je MVP.
- Webhook/Slack/e-mail je další fáze; stávající `REDLINE_WEBHOOK_URL` rozšířit o `event` a
  `project` a filtrovat, aby cizí projekt neposílal data.

### 12.5 Assignment

- `assigned_to` user id; „My tasks“ filtr; developer může přiřadit sobě/ jinému developerovi;
  tester může požádat o přiřazení.
- Assignment je metadata, ne permission; přiřazený tester stejně nesmí submitovat.

---

## 13. Bezpečnost

### 13.1 Autorizace

- Každý route s `{id}` musí ověřit projekt a akci; media/history/trace jsou nejčastější IDOR.
- Cizí/neexistující → 404 (bez rozdílu), aby nešlo enumerovat.
- `GET /api/projects` a `GET /api/sessions` filtrují na serveru; UI filtr je jen kosmetika.
- Bulk endpointy validují každý prvek; žádný „all or nothing“ bez rollbacku.
- Agent token allowlist; `list_reviews`/`list_projects` filtrují.
- Share token je capability: jen jeden task, expirace, revokace; nesmí listovat projekt.

### 13.2 Tokeny a hesla

- Hesla Argon2id, unikátní salt; žádný plaintext; timing-safe verify.
- Session ID a tokeny 32 random bytes; na serveru hash (SHA-256), prefix pro UI.
- Token se zobrazí jen jednou; žádné tokeny v logech, auditu, URL ani v chybových stránkách.
- Cookie HttpOnly, Secure (produkce), SameSite=Strict; `__Host-` prefix volitelně.
- CSRF token + SameSite + `Sec-Fetch-Site`; login rate limit; session fixation; logout revoke.
- `Referrer-Policy: no-referrer`; `X-Content-Type-Options: nosniff`; `X-Frame-Options: DENY`
  (nebo CSP frame-ancestors); HSTS na Traefiku.
- Extension CORS jen pro extension origin; PAT v hlavičce, ne v query.
- Rate limit na login, token auth failures, review create, submit; audit failed logins.

### 13.3 Soubory a data

- `valid_id`, `slugify`, canonicalize; žádný `..`; projekt slug nesmí escapovat.
- Session folder move (project change) ověřit oba projekty a cílový adresář.
- Backup: users/tokens/audit jsou v data dir; owner musí zálohovat; token hashe nejdou obnovit.
- Retention: audit 1 rok (konfigurovatelné); notifikace 90 dní; sessions expirace.
- User delete: soft delete, revoke sessions/PATs, reviews zůstávají s „deleted user“; GDPR export
  volitelně.
- **Právě jeden owner**: invariant v `server.json`; transfer je atomický; owner nemůže smazat
  sám sebe ani zůstat bez ownera. Destructive admin akce vyžadují potvrzení a re-auth.
- Komentáře a notifikace jsou vždy scoped na projekt/člena; mention cizího uživatele se odmítne.
- Notifikace nesmí obsahovat tokeny, hesla ani data z projektů, které uživatel nevidí.

---

## 14. Migrace a zpětná kompatibilita

### 14.1 Z jediného tokenu

1. Start s `users.json` prázdným / chybějícím.
2. Owner bootstrap z `REDLINE_TOKEN` (nebo setup stránky): vytvoří ownera, heslo.
3. Legacy token:
   - zůstane platný jako **owner PAT** (prefix `legacy_`) a jako **agent token** `bridge`,
   - UI ho zobrazí s varováním „full access; rotate“,
   - owner ho může revokovat po migraci klientů.
4. Existující sessions: `created_by = owner`, `created_by_name = "Legacy owner"`,
   `created_by_type = "user"`; všechny projekty owner.
5. Existující `agent-claims.json`, `agent-presence.json`, `agent-settings.json` beze změny.
6. `?t=<token>` pokračuje pro ownera, dokud token ne revoke; nové UI ho nepoužívá.

### 14.2 Desktop

- `config.json` profily s `token` zůstanou čitelné; token se použije jako legacy owner PAT.
- Při prvním otevření remote profilu app nabídne login; po loginu se token může z profilu
  odstranit (nebo zůstat pro CLI, s varováním).
- Local profil (This computer) zůstává owner-only bez loginu.
- `Servers` stránka zobrazí logged-in uživatele a roli; logout je per profil.

### 14.3 Extension

- Starý server token funguje jako legacy owner PAT (plná práva) — bezpečnostní dluh. **Nikdy ho
  nedávej testerovi/developerovi**; jinak obchází role.
- Po upgradu vytvořit PAT pro každého uživatele (tester/developer), vložit do extension, starý
  token revokovat. Extension PAT podpora je součástí MVP (fáze 1/2), ne až integrací.
- Extension UI musí umět PAT; projektový picker filtruje podle PAT. Legacy token je jen
  migrační fallback pro ownera.

### 14.4 API klienti

- `Authorization: Bearer <legacy token>` funguje jako owner do revoke.
- Nové endpointy jsou additive; staré route zůstávají, jen s ACL.
- `GET /api/sessions` tvar zůstává, přibývají pole; žádné breaking removal.
- `POST /api/sessions/{id}/annotations` pro ownera funguje jako dnes; pro testera 403.

### 14.5 Rollback

- Multi-user verze musí umět startovat i s `users.json` chybějícím (single-user režim) pro
  případ, že owner migraci nedokončí.
- Legacy token zůstává; žádná data se nemažou.
- Pokud se migrace pokazí, obnovit data dir ze zálohy a starý binary.

---

## 15. Testování a acceptance criteria

### 15.1 Testovací matice

- **Auth**: login success/failure, rate limit, cookie flags, expiry, logout, revoke, CSRF,
  session fixation, password change.
- **Role**: owner/developer/tester/viewer/agent × každá akce z matice; očekávané 200/403/404.
- **Project isolation**: user A nesmí číst task, media, history, trace, review.md, frames,
  annotated.png z projektu B; 404; ani přes přímý `id`.
- **Tester**: read all project tasks; edit own; create, annotate, request; submit 403; STOP 403;
  confirm 403; handoff 403; delete own unsubmitted; developer submitne request; audit obsahuje
  oba aktéry.
- **Owner**: invite, role matrix, disable, revoke sessions, **transfer ownership** (starý owner
  → member, právě jeden owner), single-owner invariant a self-delete protection.
- **Project required**: new task bez projektu 400/403; Inbox jen owner; tester ho nevidí.
- **Agent**: token allowlist; `list_reviews` filtr; create v cizím projektu 403; handoff report.
- **Concurrency**: dva editoři, revision 409, UI reload; draft per user.
- **Comments**: create/edit/delete/resolve; mention jen member; cizí projekt 404; rate limit.
- **Notifications**: ready/submit/reply/mention/handoff; unread count; mark read; žádný leak
  cizího projektu; retence.
- **Audit**: každá akce; žádné tokeny/hesla; filtr.
- **Share**: scoped, expirace, revoke, žádný list projektů.
- **Migration**: starý data dir, legacy token, desktop config, extension token.
- **Security**: IDOR, path traversal, CORS, query token, brute force, cookie theft.
- **E2E**: owner invite → tester login → create → request → developer login → submit → agent
  reply → tester read/comment → developer confirm/handoff → audit/notification.
- **Desktop/extension**: login/PAT, project filter, tester request; extension unit tests.

### 15.2 Acceptance criteria (MVP)

- [ ] Owner vytvoří uživatele a přiřadí mu projekt/roli; nový task vyžaduje projekt.
- [ ] Tester se přihlásí, **vidí všechny tasky projektu**, vytvoří vlastní, anotuje a požádá
      o agenta.
- [ ] Tester **nemůže** submitnout ani přes API (403), ani STOP/confirm/handoff; cizí task
      needituje.
- [ ] Developer vidí request, otevře task, upraví a odešle agentovi.
- [ ] Agent run proběhne; tester vidí výsledek; developer potvrdí/handoff.
- [ ] Tester i developer mohou komentovat a @mention; notifikace se zobrazí příjemci a nikomu
      cizímu.
- [ ] Uživatel bez projektu nevidí projekt, task, media, historii, trace ani komentáře (404).
- [ ] Owner vidí vše; matice přístupu funguje; změna role je okamžitá; **transfer ownership**
      funguje a vždy zůstává právě jeden owner.
- [ ] Audit obsahuje login, create, request, submit, confirm, handoff, komentáře, notifikace
      a změny přístupu.
- [ ] Migrace staré instalace neztratí přístup; legacy token je označený a rotovatelný.
- [ ] Cookies HttpOnly/Secure/SameSite; hesla Argon2id; tokeny hashované; žádný token v URL
      (kromě jednorázového login/share).
- [ ] Rust testy, browser e2e, extension testy, desktop build zelené.

---

## 16. Fáze nasazení

| Fáze | Obsah | Výsledek |
|---|---|---|
| **0. Design** | Tento dokument, rozhodnutí o otevřených otázkách, wireframy | schválené zadání |
| **1. Auth foundation** | users, lokální login, sessions, owner bootstrap, **transfer ownership**, migrace, owner admin, PAT/agent tokeny, auth middleware | server má identity; owner vidí data |
| **2. Roles & ACL** | Developer/Tester/Viewer, memberships, projekt povinný, projektové filtrování, submit permission, request-agent, **extension PAT**, matice v UI | tester vytváří/žádá, developer odesílá |
| **3. Kolaborace (MVP)** | komentáře, @mentions, notifikace, audit UI, revision/konflikty | **MVP hotové** |
| **4. Hloubka** | presence, assignment/My tasks, share linky, project admin, desktop login, CLI login | pohodlnější týmová práce |
| **5. Integrace** | OIDC/SSO, e-mail/Slack, 2FA | plný multi-user provoz |

**MVP = fáze 1+2+3.** Bez 1+2 není role použitelná; 3 je rozhodnutá součást MVP. Fáze 4/5
až po ověření v praxi. Odhad: 1 je zásah do auth a všech route; 2 je policy + UI; 3 je nový
datový model (komentáře/notifikace/revision); 4/5 jsou integrační.

**Release (rozhodnuto):** multi-user je **samostatný release v0.28**, ne součást v0.27. Musí mít
migrační guide, rollback a upozornění na legacy token. Nevydávat společně s jinou velkou změnou.

---

## 17. Návrhy funkcí navíc

| Priorita | Funkce | Proč |
|---|---|---|
| **P0** | **Request agent review** (tester → developer) | Bez toho tester jen vytvoří task, který nikdo neodešle. |
| **P0** | **Audit log** | U oprávnění je „kdo to udělal“ stejně důležité jako „smí to“. |
| **P0** | **PAT + service tokeny** | Extension/CLI/agent potřebují identitu, ne root token. |
| **P0** | **Comments + @mentions** | Rozhodnuto: součást MVP; diskuse tester↔developer. |
| **P0** | **Notification center** | Rozhodnuto: součást MVP; „ready“/„agent odpověděl“ nesmí zapadnout. |
| **P0** | **Revision/optimistic concurrency** | Dva lidé na jednom tasku nesmí tiše přepsat práci. |
| **P0** | **Transfer ownership** | Rozhodnuto: jeden owner s bezpečným předáním. |
| **P1** | **Viewer role** | Stakeholder/klient může číst a komentovat bez rizika. |
| **P1** | **Assignment + My tasks** | Tasky nepadají mezi lidi; tester vidí, na čem pracuje. |
| **P1** | **Presence „Jana edituje“** | Signál pro souběh; není to hard lock. |
| **P2** | **OIDC/SSO (Authelia/Cloudflare Access)** | Firemní login bez dalších hesel; pokud už SSO běží. |
| **P2** | **Share links** | Externí tester/klient bez účtu. |
| **P2** | **Project admin role** | Owner nemusí spravovat každý projekt sám. |
| **P2** | **2FA/TOTP pro ownera** | Ochrana nejvyšší role. |
| **P2** | **E-mail/Slack notifikace** | Pokud je SMTP/webhook k dispozici. |
| **P3** | **Teams/groups** | Hromadné přiřazení rolí; pro malé týmy zbytečné. |
| **P3** | **Real-time presence/cursors (SSE/WebSocket)** | Nice-to-have; polling dnes stačí. |
| **P3** | **GDPR export/delete uživatele** | Pokud to firma potřebuje. |
| **P3** | **Project archive** | Skrytí neaktivních projektů bez mazání. |

---

## 18. Rozhodnutí a zbývající otázky

### 18.1 Rozhodnuto (2026-11-03)

| Otázka | Rozhodnutí |
|---|---|
| Přihlášení | **Lokální hesla**; OIDC/SSO mimo MVP |
| Tester → agent | **Explicitní `Request agent review`** |
| Tester scope | **Čte všechny tasky projektu, edituje jen vlastní** otevřené |
| Inbox | **Projekt povinný**; Inbox jen owner |
| Owner | **Přesně jeden**, s **transfer ownership** |
| MVP | **Komentáře a notifikace** součástí MVP |
| Mazání | Owner any; developer/tester vlastní ne-submitted |
| Členství | **Owner only** |
| Invite/reset | **Jednorázové linky**, bez SMTP |
| Desktop local | **Owner-only bez loginu** |
| Extension | **PAT** |
| Pojmenování | **Task v UI**, **Review** pro agentní kolo; API názvy zůstávají |
| Agent tokeny | **Více tokenů** s vlastním allowlistem a joby |
| Share linky | **Až později** |
| Retence | Audit 1 rok, notifikace 90 dní |
| Release | **Samostatný v0.28** s migračním guide |

### 18.2 Zbývá rozhodnout

1. **Tester a agentovo vlákno/output:** read-only? (Doporučuji ano; vyplývá z „čte všechny
   tasky“.)
2. **Komentáře:** může tester komentovat cizí task? (Doporučuji ano; čte celý projekt.)
3. **Transfer ownershipu:** má starý owner dostat default developer roli na svých projektech,
   nebo jen členství bez role? (Doporučuji zachovat existující role; nový owner může změnit.)
4. **Mazání uživatele:** soft delete vs anonymizace? (Doporučuji soft delete + revoke.)

---

## 19. Rizika a mitigace

| Riziko | Mitigace |
|---|---|
| Owner se zamkne z účtu | `redline owner reset` přes filesystem; legacy token; dokumentace. |
| Chybný transfer ownershipu | Atomický `server.json`, re-auth, potvrzení, audit; cílový musí být aktivní. |
| Starý token zůstane plná práva | UI varování, expirace, rotace, audit použití. |
| Komentář/notifikace prozradí cizí projekt | Server scoped na členství; mention validace; testy 404/403. |
| IDOR na media/history | Centrální `require_review`, testy pro každý route. |
| Dva lidé přepíší anotace | `revision` + 409 + UI konflikt; presence. |
| Tester obejde UI | Server vynutí `review.submit`; e2e test 403. |
| Agent uvidí cizí projekt | Allowlist na tokenu a filtrování `next`/`list_reviews`. |
| Migrace rozbije desktop/extension | Legacy token, fallback login, testy; Postupný rollout. |
| Příliš mnoho rolí | Držet Owner/Developer/Tester/Viewer; project admin až později. |
| Výkon listování | Malý tým; filtrování v paměti; indexace není potřeba. |
| Audit roste | Rotace/retention; JSONL; owner může exportovat. |
| Ztráta session cookie | Login znovu; PAT pro automatizaci. |

---

## 20. Poznámky k implementaci

- Auth je dnes v `redline-core/src/auth.rs`; nový `policy.rs`/`users.rs` by měl být v core, aby
  ho používal server, desktop i testy.
- `AppState` dnes drží `Auth`; nahradit `AuthService` s `users`, `sessions`, `tokens`, `policy`.
- Middleware `require_token` nahradit `require_auth` + per-route `require_*`; nestačí jen
  vyměnit token za uživatele, route musí vědět, jakou akci chrání.
- `Store` už umí projekty a session folders; přidat `created_by`/`revision` do `Meta`.
- `server.json` je jediný zdroj pravdy o ownerovi; transfer musí být atomický a testovaný
  (starý owner → member, právě jeden owner).
- Komentáře patří do session složky (`comments.jsonl`), notifikace do `notifications/<user>.jsonl`;
  obojí musí být scoped na projekt a mít rate limit.
- Všechny nové soubory musí být atomicky zapisované (`write_json`) a tolerantní ke starším
  záznamům (`serde(default)`).
- UI zůstává bez build stepu; nové stránky mohou být `login.html`, `admin.html` nebo sekce v
  `list.html`/`app.js`.
- Testy: `tests/api.rs` už má harness s `AppState`; rozšířit o uživatele a role; e2e přidat
  login a project isolation.
- Dokumentace: `docs/API.md`, `docs/UI.md`, `docs/DESKTOP.md`, `docs/TESTING.md`, `PLAN.md`,
  `CHANGELOG.md`; nový `docs/COLLABORATION.md` (tento).
