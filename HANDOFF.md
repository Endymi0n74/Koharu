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

Branche de travail : `port/upstream-0.92` = `ce01723a` + 12 commits du fork rejoués (CI, release,
updater, règles projet). C'est la base du portage ; `main` n'est pas touchée.

## Replay automatique

Testé commit par commit sur `ce01723a` : **15 passent**, **110 conflitent**. Deux des 15 ont été
retirés depuis : `45a9ad0e` (sa suppression de `clamp` casse `ToolBar.tsx` en 0.92) et `d149fc04`
(`scripts/release.ts`, absent de l'amont et appelé par personne).
Le fork porte 111 commits hors docs/CI/release ; les conflits ne sont pas cosmétiques :

- `crates/koharu-translator` **n'existe plus en amont** : traduction = `crates/koharu-llm`
  (`hosted/`, `local/`) et `koharu-pipeline/src/stages/translate`. Les ~13,8 k lignes du fork
  (glossaire, fiche de chapitre, presets, typographie, prompts) doivent être re-hébergées.
- `crates/koharu-pipeline` est réécrit : `EntityId`/`Snapshot` → `PageId`/`Session`, stages
  `detection`/`ocr`/`translation` → `detect`/`recognize`/`translate` + `inpaint`, config par stage.
- Le sous-système batch (7 067 lignes, `crates/koharu-pipeline/src/batch/`) n'a aucun équivalent
  amont : il doit être ré-implémenté sur le scheduler de 0.92.
- `bun.lock`/`package.json` : purge UI du fork contre les dépendances amont.

## Liste de portage révisée (2026-11-23)

109 commits étaient en conflit. Vérifié contre `ce01723a` (lignes ajoutées déjà présentes + lecture du
code amont) : 13 sont déjà sur la branche, 15 sont écartés, **107 restent à porter**.

Déjà sur la branche (rejoués) : `3dc204bf d1f02e70 b646e34c 71d52db7 16b404e3 5b1b59e9 3606b20c`
`81c787ba ba047cbd f0793c87 d5ba3602 bff92b26`.

Écartés, avec la preuve :

| commit | sujet | pourquoi |
|---|---|---|
| `bce71ddd` | module Qwen Image 2.1 | `koharu-ml/src/qwen_image` existe en amont (274 l., `references`, sigmas Turbo) |
| `ed21f4cb` | Qwen Image 2.1 → stage inpaint | `impl Inpaint for QwenImage` dans `stages/inpaint/model.rs` |
| `ccfbc319` | tuilage VAE Qwen | `vae_tile: Some(512)` dans `qwen_image/model.rs` |
| `9cb311f9` | aligner quatre plages de deps | 0.92 est plus récent que l'alignement du fork (lucide 1.73 > 1.66) |
| `6e37ffc3` `bd9e9231` `3daa09a6` | Cargo.lock / bun.lock | à régénérer au portage, un lockfile ne se rejoue pas |
| `08e1daf1` | cargo fmt | `cargo fmt --all -- --check` sort 0 sur la branche |
| `b02f8826` | réparer les fichiers du rebase | répare une corruption propre au fork |
| `13a994f0` | fusionner deux `[profile.release]` | 0.92 n'en a qu'une |
| `6b3cf080` | `as_chunks` dans `diffusion/image.rs` | fichier absent en amont |
| `74b87651` | retirer la signature Windows | aucun `signCommand`/signpath dans 0.92 ni ses workflows |
| `4376a67e` | retirer le MSI des READMEs | doc ; la partie updater est déjà dans le commit rejoué |
| `cad84861` | clippy sur `llama/log.rs` | corrige du code introduit par `b10e9390`, porté avec lui |
| `abec9737` | clamp `ToolBar` + doublon `fr-FR` | `clamp` est requis en 0.92, le doublon n'existe pas |
| `d149fc04` | `scripts/release.ts` (préfixe `v` de git-cliff) | fichier absent de l'amont, aucun appelant ; la release passe par `.github/release-please/` |

**Vérifié comme non couvert** (l'hypothèse inverse était fausse) :

- vérification sha256/taille du téléchargement : `koharu-runtime/src/transfer/download.rs` ne lit que
  `content_length` pour la progression, aucun digest → `b19691d7` et `02d54c89` restent ;
- logs llama.cpp sur échec de chargement : `koharu-llama/src/log.rs` est un pont tracing, sans le
  tampon de lignes récentes du fork → `b10e9390` reste ;
- aperçu d'erreur dépliable : aucun `showMore`/`showLess`, `detail` reste `truncate` → `ee6cdac8` et
  `4b702c36` restent ;
- purge UI (`recharts`, `embla`, `cmdk`, `input-otp`, `react-day-picker` toujours dans
  `packages/ui/package.json`), jambes macOS encore actives, `paths-ignore` absent des workflows,
  catalogue sans Hy-MT2 ni `abliterated` : restent.

Régénérer la liste : `git log --no-merges f9c40bdd ^ce01723a --format='%h %s'`, puis retirer les
commits docs/release et les 28 hash ci-dessus.

## Lot 1 porté (2026-11-23)

Quatre commits sur `port/upstream-0.92`, `cargo check --workspace` vert après chacun :

- `bec3c4b9` `feat(store)` — `KOHARU_STORE` nomme le store avant le root Windows et le cache OS.
- `01e9fd42` `feat(llama)` — `koharu_llama::recent_logs()` : anneau des 32 dernières lignes
  WARN/ERROR de llama.cpp et ggml, jointes à l'erreur de chargement GGUF dans
  `koharu-llm/src/local/engine/load.rs`.
- `d31fa511` `feat(runtime)` — `store::Expectation` (taille + SHA-256) vérifiée avant publication ;
  `Repository::resolve` lit le tree du Hub (`/api/…/tree/<commit>?recursive=1`) pour les fichiers à
  télécharger, et vérifie chaque transfert avant le store. Test
  `a_file_must_match_its_published_size_and_digest`.
- `chore` — `/bench-profile/` et `/projects/` ignorés (les committer par accident a failli arriver).

Reste du lot 1 : les archives de `packages/*` (CUDA, ROCm, torch, llama) n'ont pas de digest publié
en dur, donc elles ne sont pas vérifiées ; un fichier déjà présent dans le store n'est pas revérifié.

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
