# PR: muse-spark extended prompt-cache retention — klaar, submit via browser

Fork: GroepOnline/jcode  ·  Branch: feat/muse-extended-cache-retention  ·  1 commit ahead of upstream master

## Wat er klaar staat
- Patch zijn gepusht naar `GroepOnline/jcode` (public fork)
- Commit: `17fd646a7` "feat(openai): enable extended prompt-cache retention for Meta muse-spark models"
- Lokaal getest: `cargo test -p jcode-base --lib provider::openai` → 2 tests groen

## Waarom de PR niet via CLI lukt
GitHub's `createPullRequest` (zowel GraphQL als REST) vereist dat de account die
de PR opent write-access heeft op het **base** repo `1jehuang/jcode`, of dat de
fork door exact die account beheerd wordt. Meerdere org-tokens (MisterWanted,
chefadmin-netizen) geven "no permission / 404". De compare-data is wél geldig
(ahead_by 1, alles staat op de org).

## Jij hoeft alleen: open deze link (geen collaborator-rechten nodig, GitHub herkent de org-fork)
https://github.com/1jehuang/jcode/compare/master...GroepOnline:feat/muse-extended-cache-retention

Klik "Create pull request", titel/body staan hieronder.

## Title
feat(openai): enable extended prompt-cache retention for Meta muse-spark models

## Body
Enables the Responses-API `prompt_cache_retention: "24h"` hint for Meta's `muse-spark`
models, matching what jcode already does for GPT-5.x.

**Why:** Meta's `muse-spark` models accept `prompt_cache_retention` on the Responses
wire API (same as GPT-5.x). jcode's `supports_extended_prompt_cache_retention` only
listed GPT-5.x, so Muse users had to export `JCODE_OPENAI_PROMPT_CACHE_RETENTION=24h`
manually to get the extended cache window. This makes it automatic.

**Change:** `crates/jcode-base/src/provider/openai.rs` adds a `muse-spark` prefix +
two unit tests.

**Verification:** `cargo test -p jcode-base --lib provider::openai` passes.

Treat as reference/proposal per CONTRIBUTING.
