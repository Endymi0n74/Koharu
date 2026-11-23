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

Testé commit par commit sur `ce01723a` : **15 passent**, **110 conflitent** (relevé de cet essai, qui
ne couvre pas la plage entière : la répartition vérifiée est plus bas). Deux des 15 ont été retirés
depuis : `45a9ad0e` (sa suppression de `clamp` casse `ToolBar.tsx` en 0.92) et `d149fc04`
(`scripts/release.ts`, absent de l'amont et appelé par personne).
Le fork porte 113 commits hors docs/CI/release (`207 - 55 docs - 24 ci - 15 chore(release)`) ; les
conflits ne sont pas cosmétiques :

- `crates/koharu-translator` **n'existe plus en amont** : traduction = `crates/koharu-llm`
  (`hosted/`, `local/`) et `koharu-pipeline/src/stages/translate`. Les ~13,8 k lignes du fork s'y
  re-hébergent : glossaire et fiche de chapitre sont faits (lot 3), les presets et la calibration
  partent avec `koharu-batch`, la typographie et les prompts restent à trancher.
- `crates/koharu-pipeline` est réécrit : `EntityId`/`Snapshot` → `PageId`/`Session`, stages
  `detection`/`ocr`/`translation` → `detect`/`recognize`/`translate` + `inpaint`, config par stage.
- Le sous-système batch (7 067 lignes, `crates/koharu-pipeline/src/batch/`) n'a aucun équivalent
  amont : il doit être ré-implémenté sur le scheduler de 0.92.
- `bun.lock`/`package.json` : purge UI du fork contre les dépendances amont.

## Liste de portage révisée (2026-11-23)

La plage `git log --no-merges f9c40bdd ^ce01723a` fait **207 commits**, et ceux-ci se répartissent
exactement, sans recouvrement :

| sort | commits |
|---|---|
| rejoués sur `port/upstream-0.92` (10 gardés, 2 retirés à la relecture) | 12 |
| écartés, preuve ci-dessous | 14 |
| restants : `docs(…)` | 55 |
| restants : `ci(…)` | 20 |
| restants : `chore(release)` | 15 |
| restants : autres `chore`/`build`/`test`/`refactor` | 13 |
| restants : `feat`/`fix`/`perf` | 78 |
| **total** | **207** |

La liste de portage se lit dans les 78 `feat`/`fix`/`perf`, mais ce n'est pas un décompte de commits :
une entrée peut en réécrire plusieurs (Qwen Image 2.1 non-Turbo n'en a aucun sur la plage), et 0.92 en
couvre déjà certains, vérifié commit par commit contre `ce01723a` (lignes ajoutées déjà présentes +
lecture du code amont). Les lots ci-dessous disent ce qui est soldé.

Rejoués sur la branche (12) : `3dc204bf d1f02e70 b646e34c 71d52db7 16b404e3 5b1b59e9 3606b20c`
`81c787ba ba047cbd f0793c87 d5ba3602 bff92b26`.

Écartés, avec la preuve (14 commits) :

| commit | sujet | pourquoi |
|---|---|---|
| `ccfbc319` | tuilage VAE Qwen | `vae_tile: Some(512)` en amont ; `params_backend`/`max_vram` et la résidence au-dessus de 20 Gio sont du réglage perf, non porté (le fit de sd.cpp place les poids) |
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
- **Qwen Image 2.1 non-Turbo, base et Uncensored** (porté par `8581e575`) : l'amont ne connaît que le Turbo
  (`unsloth/…-Turbo-GGUF`, encodeur `Qwen/Qwen3-VL-8B-Instruct-GGUF`, `SIGMAS` figés, pas de
  `num_inference_steps`, guidage seulement si `true_cfg_scale` > 1). Le fork sélectionnait deux
  checkpoints non distillés — `leejet/Qwen-Image-2.1-GGUF` Q4_K et
  `abenzerps/Qwen-Image-2.1-Uncensored-GGUF` Q4_K_M — sur l'encodeur Heretic
  (`pottokao/…-Heretic-GGUF`, `qwen3vl_8b_heretic` + son mmproj), à `--cfg-scale 6.0` et 20 pas.
  Rien de tout ça n'existe en amont. `529f67df` (l'un des 78 `feat`/`fix`/`perf` de la plage) apporte
  lui-même le
  dépôt UC, `UNCENSORED_DIFFUSION_WEIGHTS` et `load_uncensored`, mais il dépend de tout le reste de
  `bce71ddd` : `QwenImageInpaint` et son `processor`, l'encodeur Heretic, le chemin CFG 6 / 20 pas.
  Entrée de portage :
  - `koharu-ml/src/qwen_image` : les deux dépôts et l'encodeur Heretic, un `load` par checkpoint,
    et `Options` ouvert à `steps`/`sigmas` (`sigmas` vide garde ceux du scheduler,
    `cfg: Some(6.0)`) — le Turbo garde `SIGMAS` et `cfg` à 1.0.
  - `Inpainter` (forme 0.92) : deux variantes (`qwen-image`, `qwen-image-uncensored`) dans l'enum,
    `load`, `impl Inpaint`, plus `packages/ipc/src/protocol.ts` et `packages/koharu/lib/models.ts`.
    C'est `ed21f4cb` et `529f67df` réécrits, pas rejoués : t2i et édition sont déjà amont.
  - Non porté, assumé : masque natif + `strength` + `padding_mask_crop` + image de référence dans
    l'inpaint Qwen ; l'amont redessine la fenêtre entière et recolore hors masque
    (`stages/inpaint/tone.rs`).
- purge UI (`recharts`, `embla`, `cmdk`, `input-otp`, `react-day-picker` toujours dans
  `packages/ui/package.json`), jambes macOS encore actives, `paths-ignore` absent des workflows,
  catalogue sans Hy-MT2 ni `abliterated` : restent.

Régénérer la liste : `git log --no-merges f9c40bdd ^ce01723a --format='%h %s'`, puis retirer les
26 hash ci-dessus (12 rejoués, 14 écartés) et les commits `docs`/`ci`/`chore(release)`/`chore` ; il
reste les 78 `feat`/`fix`/`perf`.

## Relecture des commits rejoués (2026-11-23)

Les 12 commits rejoués ont été relus un par un sur 0.92 ; deux ne tenaient pas la route, un était
incohérent. Il en reste 10.

Retirés :

- `chore(project): remove duplicated install rule from packages/koharu/AGENTS.md` — la cible n'existe
  pas sur 0.92 : le commit créait `packages/koharu/AGENTS.md` avec le seul bloc `nextjs-agent-rules`,
  que `next dev` régénère de toute façon (le commentaire du bloc le dit lui-même).
- `build: expliciter la cible des ressources de licence` — la carte `resources` n'existait que pour
  fusionner l'overlay Windows du fork (dll torch) ; sans overlay, l'array amont suffit.

Corrigé dans `fix(ci): resolve the release changelog against the fork` :

- `cliff.toml` pointait le fork dans `[remote.github]` mais laissait le postprocessor `<REPO>` sur
  `koharu-rs/koharu` : les liens d'issues du gabarit seraient allés chez l'amont. Les deux pointent
  maintenant `Endymi0n74/Koharu`.

Vérifiés, rien à changer :

- **Updater** : clé minisign et endpoint du fork. Le bloc `windows.installMode: "quiet"` de l'amont
  disparaît au profit du défaut : c'est le « passive » que le fork avait voulu (son tauri.conf
  n'avait aucun bloc `windows`). Défaut vérifié dans la version réellement compilée —
  `tauri-plugin-updater` 3.0.0-alpha.3 @ `14ed4c4f`, `plugins/updater/src/config.rs` :
  `WindowsUpdateInstallMode::Passive` est `#[default]`, et `WindowsConfig::install_mode` est
  `#[serde(default)]`. Ni le Rust ni le TS ne posent d'`install_mode`, donc rien à écrire dans
  `tauri.conf.json`.
- **Winget** : l'amont a bien le job ; le fork le retire (pas de `WINGET_TOKEN`, et le paquet
  `mayocream.koharu` appartient à l'amont) et ne laisse qu'une note à sa place. Les deux commits du
  fork (`b646e34c` puis `71d52db7` : désactivation, puis retrait du gabarit commenté) y sont rejoués
  en un seul.
- **Sentry** : l'amont appelle `.github/sentry.sh` via `beforeBundleCommand` ; la garde
  `SENTRY_AUTH_TOKEN` + `SENTRY_ORG` évite un échec sans secrets, `SENTRY_ORG` venant des variables
  de dépôt.
- **`v` dans tags et noms de release** : l'amont était en `false` alors que le fork tague `v0.92.0` ;
  `cliff.toml` accepte désormais les deux formes de tag.
- **`CLAUDE.md`** : créé par le fork (canary + règle immuable) ; `AGENTS.md` amont ne porte pas la
  règle, donc pas de doublon.
- **`.gitignore`** : la sonde `spec_decode_probe.rs`, présente non suivie dans l'arbre. La règle
  `/memory.md` du fork a été retirée : aucun fichier de ce nom, aucun commit qui l'ait jamais suivi
  sur 0.92, et plus aucune règle qui le documente.

## Lot 1 porté (2026-11-23)

Sept commits sur `port/upstream-0.92`, `cargo check --workspace` vert après chacun :

- `1be3109b` `feat(store)` — `KOHARU_STORE` nomme le store avant le root Windows et le cache OS.
- `e46112d3` `feat(llama)` — `koharu_llama::recent_logs()` : anneau des 32 dernières lignes
  WARN/ERROR de llama.cpp et ggml, jointes à l'erreur de chargement GGUF dans
  `koharu-llm/src/local/engine/load.rs`.
- `340d20e6` `feat(runtime)` — `store::Expectation` (taille + SHA-256) vérifiée avant publication ;
  `Repository::resolve` lit le tree du Hub (`/api/…/tree/<commit>?recursive=1`) pour les fichiers à
  télécharger, et vérifie chaque transfert avant le store. Test
  `a_file_must_match_its_published_size_and_digest`.
- `chore` — `/bench-profile/` et `/projects/` ignorés (les committer par accident a failli arriver).
- `e800961c` `feat(llm)` — sonde `KOHARU_LLM_DEBUG` sur l'engine à séquences parallèles de 0.92 : une
  ligne `LLMDBG` par séquence (prompt, shape, history, first_token, step=N tok=M, text) ;
- `3d908bf9` `feat(pipeline)` — sonde `KOHARU_PATCH_HASH` dans `Member::apply` (le `SessionCommitter`
  de 0.92) : contenu des blocs de la page après chaque étage, hash des tiles du masque et du
  cleanup comprises ;
- `6050ba87` `feat(pipeline)` — `DETHASH` : digest des détections brutes (`Detected::patch_hash`),
  logué avant `layout` dans `stages/detect`.

Le lot 1 est clos. Hors couverture : les archives de `packages/*` (CUDA, ROCm, torch, llama) n'ont
pas de digest publié en dur, donc elles ne sont pas vérifiées ; un fichier déjà présent dans le store
n'est pas revérifié.

## Lot 2 (2026-11-23)

- `586ebeb8` `feat(llm)` — catalogue : `hy-mt2-7b` et `hy-mt2-1.8b` (texte seul, `markup: None`,
  `kind: TRANSLATION`), quantizations Q4_K_M/Q6_K/Q8_0 aux noms de fichiers des dépôts, 38 langues
  documentées ; le 30B-A3B reste dehors (`hy_v3` que le runtime épinglé ne résout pas).
- `afd70fd2` `feat(translate)` — défaut `koharu/hy-mt2-7b:Q4_K_M` (6/6 sur six segments japonais,
  contre 3/6 pour le 1.8B et 2/6 pour qwen3.5-4b), tableau du README du pipeline à jour.
- `8581e575` `feat(inpaint)` — Qwen Image 2.1 non distillé : `qwen_image::Checkpoint` (Turbo, base
  `leejet/Qwen-Image-2.1-GGUF` Q4_K, uncensored `abenzerps/…` Q4_K_M) sur l'encodeur Heretic Q4_K_M
  et le VAE partagé ; `Options.steps`/`sigmas` (vidé = celui du checkpoint) et 20 pas à CFG 6.0 pour
  les deux nouveaux. `Inpainter::{QwenImageBase, QwenImageUncensored}` (`qwen-image-base`,
  `qwen-image-uncensored`), `protocol.ts` régénéré et `models.ts`. Hors périmètre, assumé : masque
  natif, `strength`, `padding_mask_crop` et image de référence.

**Calibration VRAM : non portée ici**, elle n'a pas de propriétaire sur 0.92. `preset.rs`
(`estimate_vram`, `MEASURED_PEAKS`, `resolve_auto`, `MeasuredPeaks`) et `batch/calibration.rs`
servent `koharu-batch` (`--llm auto`, `--vram-budget`, `--reset-calibration`) ; l'app 0.92 laisse
llama.cpp `fit_params` placer les poids et ne publie qu'un échec de chargement avec la mémoire libre
(`gpu.rs`). `9f935abd` et `112ee4ba` vont donc avec le lot 4 ; `1b95e34b`/`94cfa4af`/`5b8eb60c` sont
soldés par les deux commits ci-dessus (le purge était déjà l'état amont, `qwen3.5-9b-abliterated`
n'existant pas en 0.92).

## Lot 3 porté (2026-11-23)

La fiche de chapitre et le glossaire, re-hébergés au découpage prévu :

- `bbe5d0d0` `feat(llm)` — `koharu-llm::profile` : `Profile` (série, synopsis, registre,
  personnages, glossaire), `Term`, `Profile::block` (les blocs « Project context » et « Glossary »
  du fork), `Profile::merge` (une paire de l'utilisateur remplace l'appris du même source, la
  dernière gagne), et `profile::learn`, qui porte l'unique appel du fork (`profile_prompt`,
  `profile_schema`, `profile`) sur rig, schéma en sortie contrainte. Trois tests : fusion, réponse
  à moitié remplie, dossier vide.
- `5d2f20ee` `feat(translate)` — `TranslatorConfig.glossary` (`Vec<Term>`) et le stage apprend le
  dossier une fois par conversation, sur tout le texte du projet en ordre de lecture (plafond
  16 000 caractères), sur un modèle local seulement ; son bloc entre dans le prompt système de
  chaque page. `protocol.ts` régénéré : `Term` et `glossary?: Term[]`.

Divergences assumées avec le fork :

- **Persistance** : le fork écrivait le dossier dans un composant de projet
  (`dev.koharu.document.profile`) et le relisait de la scène ; 0.92 n'a pas de composant pour ça,
  donc le dossier vit dans la `Conversation` et tombe dès que le modèle, la cible, les consignes ou
  le glossaire changent. Rien n'est écrit dans le projet : un projet rouvert relance l'appel.
- **PLaMo** : la branche qui privait PLaMo de fiche (son gabarit GGUF n'a pas de canal JSON) n'a
  pas d'objet ici — 0.92 ne catalogue aucun PLaMo et laisse rig rendre le gabarit.
- **Hébergé** : comme le fork, la fiche ne s'applique qu'aux modèles locaux.
- **Non porté, assumé** : la typographie du fork (`normalize_segment`, espaces françaises après
  traduction) n'a pas d'équivalent dans le stage 0.92.

Vérifié : `cargo check --workspace` 0, `cargo fmt --all -- --check` 0, 12 tests `stages::translate`
verts, 46 tests `koharu-llm` verts, `tsc --noEmit` de `packages/koharu` 0. Rien n'a été comparé à
un modèle chargé : les deux commits sont vérifiés par compilation et tests unitaires, pas par une
traduction réelle.

## Ordre de portage proposé

1. Socle infra, faible couplage : `KOHARU_STORE`, vérification sha256/taille du store, logs
   llama.cpp, probes `KOHARU_LLM_DEBUG`/`KOHARU_PATCH_HASH`.
2. Catalogue de modèles et Qwen Image 2.1 non-Turbo : faits ; la calibration VRAM part avec le
   lot 4.
3. Glossaire et fiche de chapitre dans `koharu-llm` + `stages/translate` : faits (lot 3) ; la
   typographie du fork reste une entrée à part.
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
