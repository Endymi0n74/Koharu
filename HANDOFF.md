# Handoff — KoharuBatch (fork Endymi0n74/Koharu)

## Objectif

Renommer le fork en KoharuBatch pour ne plus le confondre avec `koharu-rs/koharu`, puis publier
`0.92.0`. Décision de l'utilisateur (2026-11-23) : **reporter le fork sur l'amont 0.92 d'abord**,
rien n'est publié avant que l'arbre compile.

## État bloquant

`main` = `origin/main` = `a8387aab` sort du merge `466f7600` (parents `f9c40bdd`, fork 0.89 ;
`ce01723a`, amont 0.92.0), commité **sans résolution** : 171 marqueurs de conflit dans 56 fichiers
suivis, dont `Cargo.toml`, `package.json`, `bun.lock`, 20 locales, `crates/koharu-pipeline`,
`crates/koharu-ml`. Rien ne compile ni ne s'installe : ne rien publier depuis `main`, et ne pas
taguer cette arborescence.

Branche de travail : `port/upstream-0.92` = `ce01723a` + 13 commits du fork rejoués (CI, release,
updater, règles projet). C'est la base du portage ; `main` n'est pas touchée.

## Replay automatique

Testé commit par commit sur `ce01723a` : **15 passent**, **110 conflitent**. L'un des 15
(`45a9ad0e`) a été retiré depuis : sa suppression de `clamp` casse `ToolBar.tsx` en 0.92.
Le fork porte 111 commits hors docs/CI/release ; les conflits ne sont pas cosmétiques :

- `crates/koharu-translator` **n'existe plus en amont** : traduction = `crates/koharu-llm`
  (`hosted/`, `local/`) et `koharu-pipeline/src/stages/translate`. Les ~13,8 k lignes du fork
  (glossaire, fiche de chapitre, presets, typographie, prompts) doivent être re-hébergées.
- `crates/koharu-pipeline` est réécrit : `EntityId`/`Snapshot` → `PageId`/`Session`, stages
  `detection`/`ocr`/`translation` → `detect`/`recognize`/`translate` + `inpaint`, config par stage.
- Le sous-système batch (7 067 lignes, `crates/koharu-pipeline/src/batch/`) n'a aucun équivalent
  amont : il doit être ré-implémenté sur le scheduler de 0.92.
- `bun.lock`/`package.json` : purge UI du fork contre les dépendances amont.

## Ordre de portage proposé

1. Socle infra, faible couplage : `KOHARU_STORE`, vérification sha256/taille du store, logs
   llama.cpp, probes `KOHARU_LLM_DEBUG`/`KOHARU_PATCH_HASH`.
2. Catalogue de modèles et calibration VRAM.
3. Glossaire et fiche de chapitre dans `koharu-llm` + `stages/translate`.
4. `koharu-batch` (cli, run, report, cbz, prune, calibration) sur le scheduler 0.92.
5. App : mode dossier, locale `fr-FR`, purge UI, erreurs inline.
6. Renommage KoharuBatch, montée `0.92.0`, tag `v0.92.0`, publication.

## Décisions ouvertes

1. **Périmètre du renommage.** Retenu pour l'instant : identité produit (nom affiché, fenêtre,
   README/docs, métadonnées) ; les crates `koharu-*` et les modèles HuggingFace gardent leur nom.
   Reste à trancher : binaires (`koharu.exe`, `koharu-batch.exe`), identifiant Tauri `Koharu`,
   dépôt GitHub. Les binaires changent les noms d'assets, l'endpoint updater et les commandes
   documentées ; l'identifiant Tauri casse la continuité de mise à jour depuis 0.89.
2. Référence de version : `[workspace.package] version` de `Cargo.toml` reste la seule source,
   tag et nom de release préfixés `v` par release-please.

## Chaîne de build validée (2026-11-23)

- `bun install --frozen-lockfile --ignore-scripts` : 149 paquets, `bun.lock` (v2) intact.
- `cargo check --workspace` : OK (`Finished dev profile` en 1 min 07).
- `tsc --noEmit` sur `packages/koharu` : OK, après suppression du `.next` hérité de `main`.
- Commit rejoué `45a9ad0e` (« deux séquelles du rebase ») **retiré** : sa suppression de `clamp`
  dans `ToolBar.tsx` casse `BrushSize` en 0.92, et sa correction `fr-FR` est déjà en amont.
- `bun install` complet lance le `postinstall` (`cargo install tauri-cli --git … --locked`), installé
  sous `D:\codex\tools\cargo` via `CARGO_INSTALL_ROOT`.

## Environnement

- Aucun outil Rust/JS dans le PATH du shell ; utiliser :
  `C:\Users\endymion\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin`,
  `D:\codex\tools\bun\bun-windows-x64\bun.exe` (1.4.5 : le 1.3.14 de `~/.bun` ne lit pas
  `bun.lock` v2 et le réécrit en v1),
  `D:\codex\tools\ninja` (`cef-dll-sys` configure CMake avec `-G Ninja`).
- `rg` absent : chercher avec `git grep` ou l'outil de recherche de l'agent.
- `gh` authentifié (Endymi0n74, scopes `repo` + `workflow`) ; toujours `-R Endymi0n74/Koharu`.
- Le tag `v0.89.0` est propre (aucun marqueur) : c'est l'état de référence qui buildait.
