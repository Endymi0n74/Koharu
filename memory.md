# Koharu — mémoire de projet

Fork de [koharu-rs/koharu](https://github.com/koharu-rs/koharu) (traducteur de manga ML, Rust).
Remotes : `origin` = https://github.com/Endymi0n74/Koharu (branche `main`), `upstream` = koharu-rs/koharu.

Machine de référence : **RTX 3070 8 Go (sm_86)**. Store : `%LOCALAPPDATA%\koharu\packages`
(`D:\koharu\store` pour les runs e2e réels : `--store D:/koharu/store`).

Règles durables : [`AGENTS.md`](AGENTS.md). Ici : état du projet, décisions tranchées, pièges.

## Livré (sessions récentes)

- **CLI `koharu-batch` élaguée : rubriques d'aide puis sous-commandes** (rupture assumée, aucun
  alias — AGENTS.md interdit la rétrocompatibilité) : `--help` passe de 35 drapeaux en vrac à 5
  rubriques (Options / Model & VRAM / Pipeline / Run control / Reproducibility) + section
  Commands, et les 4 drapeaux one-shot deviennent les sous-commandes **`models`**,
  **`prune [--delete]`**, **`reset-calibration`** — le flux de traduction reste plat
  (`koharu-batch --input …` n'a pas de mot-clé). `--store` est global (`prune --store DIR`, après
  le nom) et `args_conflicts_with_subcommands` **refuse** tout mélange drapeau de run +
  sous-commande au lieu de l'ignorer en silence (refus testé des deux côtés du nom). Migrés sans
  exception : `koharu-batch.yml` (smoke `models`, étape renommée « prune »), `test-prune-step.ts`
  (sélecteur `prune`), README/README.en, `docs/en/fork.mdx` (options, housekeeping, codes de
  sortie), messages d'erreur (`koharu-batch models`). Contrat d'exit inchangé : 0 = tout a réussi,
  1 = échec, **2** = ligne refusée par clap ; les anciens drapeaux → 2. Vérifié : fmt/clippy verts,
  141 tests lib + 2 binaire, smokes rejoués (dry-run 0, `models` 0, prune liste puis supprime 0,
  mélanges 2, sans arg 1, extraction `test-prune-step.ts` OK).
- **Prompt enrichi** (`crates/koharu-translator/src/prompt.rs`) : `source_guidance` (JA/RU) +
  `target_style` (FR) — seul levier retenu pour la qualité JA/RU→FR (échantillonnage et pivot EN
  écartés). 81/81 tests translator.
- **`koharu-batch`** : logique déplacée du binaire vers la lib (`crates/koharu-pipeline/src/batch/`
  : `cli.rs`, `run.rs`) — **115 tests lib** exécutés par CI (`cargo test --tests` n'exécute pas
  les tests d'une cible binaire). Exécution **phase-major** ; exit 0 = toutes les pages réussies.
- **Robustesse batch** : `--retries` (défaut 1) par stage, rapports réécrits à chaque phase/page
  (checkpoint), toutes les erreurs de finalisation restent des échecs page, reprise CBZ avec
  report des entrées déjà traduites (`carried`) et extension de l'entrée lue dans les octets
  réels (fin du bug JPEG-nommé-PNG), `finish()` avant rename (Windows).
- **Fonctionnalités batch** : `--pages` (plage 1-based), `--recursive`, `--quiet`, progres
  `[i/n] stage · Xs · ETA`, **mode volume** (dossier sans images au premier niveau → un chapitre
  par sous-dossier contenant des images et par .cbz ; un dossier de regroupement sans images
  descend jusqu'aux chapitres — `Vol/Ch1/p.png` = chapitre `Vol/Ch1` —, sorties miroir sous
  `--output`, rapports par chapitre + résumé global, phase-major sur tout le volume),
  **`--json <PATH>`** (résumé machine : ok/compteurs/
  chapitres/pages/échecs, écrit aussi en dry-run).
- **Perf batch** : finalisation (encode/écriture/vignette) sur un **thread worker** qui chevauche
  la traduction suivante (le render reste sur le thread principal — il emprunte la session) ;
  archive CBZ d'entrée ouverte **une seule fois** (`cbz::ArchiveReader`) au lieu d'une fois par
  page. Mesures : voir *Mesures* plus bas.
- **Rapport de reprise** : les pages ignorées d'une reprise gardent durées, stages et vignettes
  du run d'origine — état `.report.state.json` par chapitre écrit avec chaque rapport
  (fixtures e2e : run `--overwrite` puis reprise, 5/5 ignorées avec données intactes).
- **App desktop** : locale **fr-FR** complète (434 clés, tests de parité sur les 10 locales) +
  `languages.*` ; **mode dossier** (« Open a folder… » importe un dossier comme projet via
  `ProjectLibrary::folder_project_name`) ; `JobGuard` (Drop retire stop+job, panique → Failed).
- **CI** : step *Typecheck UI* dans `lint.yml` + `typecheck` dans `@koharu/app`, smoke tests
  `--dry-run` **et mode volume** (dossier `ch1` + sous-dossier imbriqué `Vol/Ch2`, assertion sur
  les labels du JSON) dans `koharu-batch.yml`.
- **Release** : `lto = "thin"` dans le profil release ; tag **`v0.83.5`** publié ; job **macOS
  désactivé** dans `release.yml` (secrets Apple absents) — réactiver quand
  `BUILD_CERTIFICATE_BASE64` / `KEYCHAIN_PASSWORD` seront configurés.

## Décisions tranchées (ne pas rouvrir sans re-mesurer sur la cible)

- **Règle du mode volume** : ≥1 image au premier niveau ⇒ un chapitre (sous-dossiers ignorés) ;
  `--recursive` ⇒ arbre entier aplati en un chapitre ; sinon sous-dossiers + .cbz = volume ;
  rien ⇒ refus avec message expliquant les deux cas. Imprimée par `--dry-run`. Dossiers
  **imbriqués** : un dossier sans images directes mais avec des sous-dossiers est un dossier de
  regroupement et **descend** jusqu'aux chapitres (`Vol/Ch/p.png` → chapitre `Vol/Ch`, labels =
  chemins relatifs, tri naturel) ; un dossier avec des images reste un seul chapitre listé
  récursivement. `/` conservé dans les sorties miroir, remplacé par `-` dans les bases
  `--report`, et le dédoublonnage compare la forme rapport (`Vol/Ch1` vs `Vol-Ch1` ⇒ second
  renommé `Vol-Ch1-2`).
- **Rapports de reprise** : état `<base>.report.state.json` écrit à chaque rapport
  (`report::save_state`, fusion par index — les pages hors `--pages` survivent) ; une reprise
  réécrit ses lignes « ignorée » avec durées/stages/vignettes du run d'origine (`PageReport::
  previous`, appariement index **+ nom** pour ne jamais accoler des données périmées).
  `--report none` n'écrit ni rapport ni état. JSON inchangé : statut = ce run.
- **Rapports** : base = sortie sans `.cbz`/`.zip` + `.report` poussé littéralement → fichiers
  `<chemin>.md`/`.html` même avec des points dans les noms (`vol.1` → `vol.1.md`, pas `vol.md`,
  et pas de collision `ch.1`/`ch.2`). Pas de rapport HTML global en volume : rapports par
  chapitre + résumé stderr + `--json`.
- **`--pages` en volume** : s'applique à **chaque** chapitre, doit tenir dans le plus petit
  (erreur stricte, pas de troncature silencieuse).
- **Overlap finalisation** : seul encode/écriture/vignette part sur le worker. Le render est
  exclu (session empruntée en `&mut` par l'exécution de traduction suivante — conflit de
  borrow, pas seulement une question de `Send`). Un abort (Drop du worker) envoie `Abort` :
  l'archive partiellement écrite est supprimée, jamais publiée.
- **Mesures avant/après** : voir *Mesures* — le bruit de génération LLM domine ; le gain
  structurel est réel mais < 1,5 s par chapitre de 6 pages sur cette charge.
- **Flash attention** : défaut binaire `-1 (AUTO)` déjà actif sur CUDA. Le switch
  `KOHARU_FLASH_ATTN` a été **retiré** (mesure ON vs AUTO = bruit).
- **Réutilisation prefill/KV** : **non adoptée.** Mesuré : plafond ~12-15 % (préfill 1,52 s +
  1,40 s de retry sur un stage de 10,49 s, génération 4,25 s, ~3,3 s d'overhead), risque de sortie
  silencieusement fausse, budget 8 Go, concurrence des stages. Instrumentation retirée après mesure.
- **GUI pour `koharu-batch`** : **non** — CLI headless + app Tauri `koharu-app` existent déjà ;
  ne pas créer de 2ᵉ interface.
- **Exemple `crates/koharu-llama-sys/examples/flash_attn_default.rs`** : conservé (documente le
  défaut AUTO, aucun coût).

## Mesures (2026-09-25, RTX 3070, `gemma4-e4b-it` Q4_K_XL, `--no-calibration`, warm)

Charge : 6 pages 770×1080 (fixture object_detection), avant = binaire buildé depuis HEAD
`4466b975`, après = working tree (worker finalizer + archive ouverte 1×).

| Run | avant | après |
|---|---|---|
| dossier → dossier PNG (rep 1) | 86,2 s (génération longue : 70,3 s de traduction) | 75,6 s |
| dossier → dossier PNG (rep 2) | 74,6 s | 73,3 s |
| cbz → cbz | 69,3 s | 70,1 s |
| overhead hors stages (load+render+finalisation+rapport) | ~0,13 s/page | ~0,10 s/page |

**Conclusion honnête (petites charges)** : la variance de génération du LLM (±10 s sur des
pages identiques) noie le gain dans le bruit total ; décomposé, la finalisation (encode PNG
~0,2-0,3 s/page + écriture + vignette) sort du chemin critique et le coût par page mesuré est
cohérent, mais le gain mur est < 1,5 s par chapitre de 6 pages sur cette petite charge.
E2E volume réel (3 chapitres, 4 pages) : 47 s, sorties + rapports par chapitre + JSON, exit 0.

### Vrai CBZ 50 pages (2026-09-25, idem + `real50.cbz`)

Charge : 50 pages extraites de *Dragon Ball Full Color Vol. 01* (1662×2560 couleur, 31,5 Mo),
CBZ → CBZ, `gemma4-e4b-it`, avant = HEAD `4466b975` (rebuild dans le worktree
`koharu-bench-before`), après = worker finalizer. **Banc croisé rejoué par `bench-cross.ts`**
(2026-09-25 22 h 48 → 09-26 00 h 26 ; binaires reconstruits avec
`cargo build --release --locked -p koharu-pipeline --bin koharu-batch`) : chaque binaire dans
les deux ordres (cooldown 600 s). 4 runs, tous exit 0, 50/50 traduites, archives ~202 Mo
valides, modèle épinglé identique sur les 4 runs ; chiffres ci-dessous régénérés par le script.
Artefacts : `%TEMP%\koharu-bench-cross\{summary.md,runs.json,o1-*,o2-*}`.
**Banc rejouable** : `bun scripts/bench-cross.ts --before <ancien> --after <nouveau>
--input <cbz|dossier> --store D:/koharu/store` — exécute les 2 ordres, épingle le modèle
résolu (la dérive de VRAM libre fait changer `auto` en cours de session), refuse tout
argument hors `--no-calibration`, persiste dans `runs.json` (2 sessions `--orders 1` puis
`--orders 2` fusionnent) et écrit `summary.md` (résidus par slot froid/chaud + gain
ordre-neutralisé). `--plan` pour vérifier sans traduire, `--pause` pour le cooldown entre
les ordres (défaut 600 s).

| Ordre d'exécution | mur | Σétapes | résidu `mur − Σétapes` |
|---|---:|---:|---:|
| ordre 1 : avant (froid) → après (chaud) | 1681,8 → 1246,3 s | 1648,1 → 1229,3 s | 33,7 → 17,0 s |
| ordre 2 croisé : après (froid) → avant (chaud) | 1179,0 → 1205,1 s | 1162,3 → 1183,7 s | 16,7 → 21,4 s |

| Résidu par binaire | 1er exécuté (froid) | 2e exécuté (chaud) | moyenne | par page |
|---|---|---|---|---|
| avant `4466b975` | 33,7 s | 21,4 s | 27,5 s | 0,55 s |
| après (finalizer) | 16,7 s | 17,0 s | 16,8 s | 0,34 s |

- **L'écart brut varie de −435,5 s à −26,1 s selon l'ordre** : l'ordre 1 (avant froid →
  après chaud) éclate à −435 s, l'ordre croisé (après froid → avant chaud) ne donne que
  −26 s — session de nuit plus chargée (Σétapes de 1162 à 1648 s sur les 4 runs) et la
  position domine : runs en 1er = 1405,2 s de Σétapes en moyenne contre 1206,5 s en 2e
  (**−198,7 s de warm-up**). Aucun de ces écarts bruts n'est le gain.
- **Gain structurel ordre-neutralisé = −10,7 s** (résidu moyen 27,5 → 16,8 s) ≈ **0,21 s/page**,
  stable au bruit près (ancien banc manuel : −10,1 s / 0,20 s) : le warm-up gonfle le résidu
  « avant » (33,7 → 21,4 s) alors que le résidu « après » reste stable quel que soit l'ordre
  (16,7 / 17,0 s). La finalisation (~0,3 s/page : encode PNG du render 1662×2560 + écriture +
  vignette) sort du chemin critique **et** l'hors-étapes devient prévisible.
- Σétapes moyennes position-neutralisées : avant 1415,9 s vs après 1195,8 s (−220,1 s) —
  direction favorable, mais n=2/cellule et session chargée : à confirmer avant tout chiffre public.
- Le **render (~2,2 s/page, déduit du gap inline 2,56 s/page) reste sérialisé** dans les deux
  binaires (borrow session) — c'est le plafond annoncé.
- Ouverture CBZ 1× vs 50× : < 0,1 s sur 50 entrées — invisible, comme prévu.
- **Argument public** : « ~0,2 s/page de finalisation sortent du chemin critique ;
  l'hors-étapes tombe à ~17 s par 50 pages et ne dépend plus de l'ordre » — pas « −15 s »,
  « −211 s » ni « −435 s » : ces écarts bruts ne se reproduisent pas.

## Non-déterminisme batch — stratégie et état (2026-09-28, à reprendre)

But : deux runs `koharu-batch` identiques doivent produire des PNG bit-à-bit identiques.
Sonde en deux niveaux, dans le même log stderr :

1. **`KOHARU_PATCH_HASH=1`** → ligne `PATCHHASH <hex> entries=N` par page × étage (hash
   DefaultHasher de la scène commitée : géométrie, textes, blake3 des blobs ; ids exclus) —
   `batch::run::log_patch_hash`, commité.
2. **`DETHASH page=<id> <hex>`** quand la variable est posée : hash de la sortie **brute** du
   détecteur RF-DETR (label/score/bbox/aire/masques) avant montage de scène —
   `stages/detection.rs::log_detector_output_hash` (commité — instrument permanent,
   voir le sort des sondes en « À faire plus tard »). Distingue
   inférence vs post-proc (`build_patch`).

**Protocole** : 2 passes A/B `--overwrite` (`--deterministic --torch-fp32 --no-calibration`),
stderr → `target/val-{a,b}.patchhash.log` ; mapper page × étage par ordre des lignes
([1/3]=page1…) ; `md5sum` des PNG pour la mesure finale. Verdict type : stable à l'étage i,
divergent à i+1 ⇒ l'opérateur entre les deux est en cause.

**Trouvé (2026-09-28)** : divergence née dans l'inférence détection ; cause = engines de conv
cuDNN split-K à atomiques choisis par heuristique **même avec benchmark=off et en fp32**. Fix =
`setDeterministicCuDNN(true)` dans `koharu_ml::determinism::enforce()` (symbole privé
`?setDeterministicCuDNN@Context@at@@QEAAX_N@Z`, présent dans torch_cpu.dll v2.13.0.7).
Résultat : 3/3 DETHASH + 9/9 PATCHHASH vision (detection/ocr/inpainting) identiques sur 3
passes ; PNG 2/3 identiques — le résidu est l'étage translation (2/12 PATCHHASH LLM).
**Coût du fix : nul détectable** — banc croisé `bench-cross.ts` (1 page, 2 ordres, 2026-09-28)
: détection 1,2–1,3 s dans les 4 runs (before `4c1cd689` vs after `f67193ed`), ocr/inpainting
idem ; résidus moyens 5,1 s vs 5,1 s. Les engines déterministes ne coûtent rien ici.

**Impasse à ne pas refaire** : forcer les backends SDP sur `math` (setSDPUseFlash/MemEfficient/
CuDNN/FA3 = false) est déterministe mais matérialise la matrice d'attention pleine (DINO,
9 216 tokens ⇒ plusieurs GiB retenus par le caching allocator) ⇒ le LLM gemma 3,9 GiB ne
charge plus (« invalid vector subscript », 0 MiB free). Backends fusionnés laissés activés,
justification dans le doc de `determinism.rs`.

**Recette d'un runtime ggml déterministe (étude 2026-09-28)** : le package vient de
`koharu-rs/llama` (workflow `release.yml` : checkout du **dernier** release llama.cpp incluant
préreleases → `cmake -S cmake` avec `-DLLAMA_CPP_SOURCE_DIR=… -DLLAMA_BACKEND=cuda` — la
recette vit dans le CMakeLists de ce dépôt fork, les options ggml passent par `-D…`).
Upstream : le PR ggml-org/llama.cpp#16016 (`-DGGML_DETERMINISTIC=ON` + `--deterministic`)
n'est **pas** fusionné ; la position mainteneur refuse les garanties bit-à-bit. Dans b10903,
les options cmake pertinentes : `GGML_CUDA_FORCE_MMQ=ON` (remplace les GEMM cuBLAS par les
kernels MMQ à ordre de réduction fixe — LE candidat pour notre flip de décodage),
`GGML_CUDA_FORCE_CUBLAS=ON` (mutuellement exclusif, à éviter), `GGML_CUDA_GRAPHS=OFF`
(défaut déjà OFF, notre log dit « CUDA graphs disabled »), `GGML_CUDA_NO_VMM=ON`
(supprime la variance d'adressage VMM), `GGML_CUDA_FA_ALL_QUANTS` sans effet déterministe.
Le build local est impossible ici (pas de toolchain MSVC/nvcc sur ce poste) — passer par un
fork du dépôt `koharu-rs/llama` avec l'option ajoutée au CMakeLists, ou un build manuel
suivant release.yml. Coût attendu : MMQ peut être légèrement plus lent que cuBLAS sur
certains shapes ; à re-mesurer avec bench-cross.ts après pivot.

**Périmètre des flags** : `--deterministic` (batch) = température LLM 0 uniquement ; le volet
vision = la recette `koharu_ml::determinism` (fp32, CUBLAS_WORKSPACE_CONFIG hérité au restart,
cuDNN benchmark off, TF32 off, désormais cuDNN déterministe).

**Résidu translation (bissécté 2026-09-28, non corrigeable in-repo)** : sonde `KOHARU_LLM_DEBUG=1`
dans `koharu_ml::llm::model` (mêmes pattern qu'OCRDBG) : `LLMDBG prompt/shape/history`
(tokenisation), `first_token` (logits du prefill), `step=N tok=M` (chaque token décodé),
`text` (sortie). Constats : prompts et prefill identiques entre process ; sampler déterministe
(greedy sous `--deterministic`, sinon `dist(299_792_458)`) ; le décodage diverge à un **pas
variable** (ex. step 84/250) par un near-tie greedy — signature d'une course d'atomiques dans
les kernels CUDA ggml du runtime prébuildé (`packages/llama/b10903`, DLL dynamiques, pas de
rebuild possible ici ; GGML_CUDA_FORCE_MMQ est un flag de compilation).
**Quantification (10 runs consécutifs, corpus val-in, 40 appels, analyse
`scripts/flip-study.py`)** : 109 paires comparables à prompt identique, **43 % des
paires divergent en cours de décodage**, **1,65 flips/1 000 tokens générés** ; `first_token`
identique 40/40 (prefill déterministe) ; l'appel court (15 tokens) stable 10/10 — le risque
croît avec la longueur ; les pas de divergence tombent sur des hotspots récurrents (≈5, 58,
83, 91, 122, 210, 242) où le modèle est en near-tie. Un appel de 150-250 tokens a ~40-60 %
de chances de différer d'un run à l'autre.

**Expérimentation MMQ (2026-09-29, fork buildé + A/B 10+10 runs) — hypothèse GEMM
falsifiée** : la recette ci-dessus a été exécutée de bout en bout. Fork `Endymi0n74/llama`
(branche `deterministic-mmq`, commit 600a5eb) : `GGML_CUDA_FORCE_MMQ=ON` +
`GGML_CUDA_NO_VMM=ON` dans `cmake/CMakeLists.txt`, et `release.yml` étendu d'un input
de dispatch `tag` pour épingler llama.cpp à `b10903` (sinon le workflow résout le dernier
release upstream — un A/B propre impose le même tag). Build CI Windows CUDA (run
`36474902343`, artifact téléchargé via `gh run download` sans attendre le job release),
swap des DLL dans `packages/llama/b10903/windows-cuda` (le check `complete()` du store ne
vérifie que la présence des fichiers, pas les hash — le swap en place passe ; baseline
sauvegardée dans `target/llm-ab/baseline/`, build MMQ dans `target/llm-ab/mmq/`). A/B 10
runs chacun, protocole LLMDBG identique, analyse `scripts/flip-study-ab.py` (groupement
par prompt hash, all-pairs) : **A (cuBLAS, mesure du jour) 104 paires / 50 divergentes
(48 %) / 3,65 flips/1000 tokens ; B (MMQ) 112 paires / 47 divergentes (42 %) / 3,25
flips/1000 — aucun changement significatif**. `first_token` identique 40/40 dans les deux
conditions (prefill stable, même hash entre les deux builds). Mêmes hotspots de
divergence (242, 4, 19, 58, 83…). **Lecture : le split-K atomique des GEMM quantisés
cuBLAS n'est pas le fautif** — MMQ + NO_VMM ne réduisent pas le flip rate ; le
non-déterminisme du decode loop vit ailleurs (attention FA, softmax/RMSNorm, ou réduction
dépendante du batch — exactement le périmètre du PR #16016 : RMSNorm/MatMul/Attention
batch-invariants). Coût perf mesuré : B ~+30 % en médiane (≈70 s → ≈93 s par run 3 pages,
queue jusqu'à 410 s sous charge GPU) — MMQ est plus lent pour zéro gain de déterminisme.
Les DLL de base (md5 `d0932411…`/`1258f638…`) sont restaurées dans le store. Pistes
suivantes, par ordre de coût : (1) build `-DGGML_CUDA_FA=OFF` pour isoler l'attention ;
(2) patcher le fork avec le diff #16016 et builder `-DGGML_DETERMINISTIC=ON` ;
(3) accepter le résidu et verrouiller la reproductibilité au niveau texte (post-proc).

**Expérimentation FA=OFF (2026-09-29, fork branche `no-fa` + A/B 10+10 runs) — hypothèse
attention falsifiée** : build CI avec `GGML_CUDA_FA=OFF` (commit `027d816`, branche `no-fa`
dérivée de `8d7b898` + input `tag`, SANS FORCE_MMQ/NO_VMM pour rester sinon identique à la
baseline ; run `36523026500`, ~1 h 35). Mécanique vérifiée dans b10903 : `GGML_CUDA_NO_FA`
→ `get_best_fattn_kernel` renvoie NONE → `ggml_cuda_flash_attn_ext_supported()` = false →
CUDA refuse FLASH_ATTN_EXT dans `supports_op`, le CPU l'accepte, et `resolve_fused_ops`
désactive proprement `flash_attn` au premier batch (log : `layer 0 is assigned to device
CUDA0 but Flash Attention is assigned to device CPU` → `Flash Attention not supported, set
to disabled`) — fallback MUL_MAT, pas d'abort. Preuves en run : llama.dll 109 Mo vs 153 Mo
baseline (~44 Mo de cubins FA en moins, md5 `658df07c…`/`a09ba877…`), et log warmup
`WARNING: flash attention not supported by CUDA0`. Cache KV identique entre conditions
(44/110 MiB, f16 K/V) → pas de cofonde quantization. Protocole identique aux runs MMQ,
passe de contrôle A' le jour même : **A' (baseline) 103 paires / 55 divergentes (53,4 %) /
3,94 flips/1000** (vs 3,65 la veille, reproductible) ; **B' (FA=OFF) 97 paires / 49 (50,5 %)
/ 3,85 flips/1000 — aucun changement significatif** ; cross A'/B' 3,87, cross B'/A(hier)
3,74 — le build FA=OFF décode au même niveau de variance que la baseline, la divergence est
indépendante du chemin d'attention. `first_token` hash `c26b7faafaa72074` identique 40/40
dans les deux conditions. Mêmes hotspots (91, 15, 19, 169, 242…). **Lecture : les kernels
FlashAttention CUDA ne sont pas le fautif non plus** — après GEMM (MMQ falsifié) puis
attention (FA falsifié), le non-déterminisme du decode loop se réduit aux suspects restants :
softmax/RMSNorm batch-dépendants, ou courses d'atomiques transverses (réductions non-GEMM).
Le périmètre exact du PR #16016 (RMSNorm/MatMul/Attention batch-invariants via
`-DGGML_DETERMINISTIC=ON`) devient la piste principale. Coût perf FA=OFF négligeable en
médiane (runs ~4 min sous charge GPU, ~70 s au calme, comparable à la baseline). DLL
baseline restaurées dans le store (md5 vérifiés) ; build FA=OFF conservé dans
`target/llm-ab/no-fa/`, logs `target/llm-ab/{a2,b2}/`. Piège du poste : les runs batch ne
survivent pas au timeout de 600 s de l'outil terminal — lancer chaque série via
`nohup bash <script> & disown` (script qui boucle les 10 runs avec log par run et
`progress.txt`), puis sonder par cycles de sleep.

**Cause racine identifiée (2026-09-29, bisection orchestration vs kernels) — la variance
naît dans la détection layout, pas dans les kernels LLM** : après FA=OFF falsifié, le portage
du PR #16016 a été écarté (PR orphelin : créé et abandonné le 2025-09-15, 9 commits jamais
mergés ; ~250 commits de divergence entre sa base `b907255f` et `b10903`, dont des refontes
majeures des fichiers FA — XOR swizzle, GGML_FA_QUANTS, sparse-fa ; diff sauvegardé dans
`/tmp/pr16016.diff`). À la place, discriminateur orchestration vs kernels : (1) contrôle GPU
au calme `target/llm-ab/a3/` — **3,75 flips/1000**, même niveau que sous charge → contention
falsifiée aussi ; (2) série CPU (`--cpu`, kernels ggml-cpu déterministes) `target/llm-ab/cpu/`
— **2,87 flips/1000, mêmes hotspots (15, 242)** → les kernels LLM ne sont pas la source ;
(3) série CPU `--no-vision` `target/llm-ab/cpunv/` — **0,00 flip/1000** (36 paires
intra+cross bit-identiques, cache OCR constant) → le pipeline texte est déterministe, le
bruit vit dans la branche vision ; (4) runs GPU avec `KOHARU_PATCH_HASH=1` (hache les bytes
blake3 de l'asset `source` après chaque étage) `target/llm-ab/gpuhash/` — la cascade est
mesurée : **DETHASH (détection RFDetR-SEG) varie déjà run-to-run** (pages 2-3 : r01 ≠
r02=r03 ; page 1 constante) → premier PATCHHASH post-détection divergent → image inpaintée
LaMa divergente (PNG écrasant l'asset `source`, non caché, ni haché) → OCR cropé divergent →
traductions différentes. Les paires de runs dans la même session (r02-r03) matchent
exactement (0/6) — la variance est groupée par session GPU/driver (méme signature que le
contrôle cuDNN du 2026-09-28). Lecture d'ensemble : les flips LLM sont l'aval de la
détection layout ; les corrections candidates côté détection : seed des opérations Torch,
NMS/anchors/resize non déterministes, batch des pages ; sondes DETHASH/PATCHHASH requièrent
respectivement `KOHARU_PATCH_HASH` (les deux — DETHASH est émis depuis
`log_detector_output_hash`, même var d'env). Le prompt LLM (`LLMDBG prompt` = texte) et
`history` (tokens) restent stables entre runs — c'est l'image (ViT/mtmd + crops) qui porte
la variance. **Le patch du runtime llama (#16016 ou équivalent) est inutile pour ce bug.**
Prochaine action utile : instrumenter/figer la détection (seed, mode eval, algorithme NMS
déterministe) — et vérifier si le fix cuDNN du 2026-09-28 s'applique au modèle RFDetR (le
knob `setDeterministicCuDNN` pourrait réduire la variance session-to-session mais pas les
hotspots intra-session).

**Instrumentation DETTRACE de RFDetR-SEG (2026-09-29, 4 sessions / 15 runs) — variance
localisée aux GEMM cuBLAS du backbone DINO, intermittente par session** : nouvelle sonde
`KOHARU_DETTRACE=1` (`crates/koharu-ml/src/koharu_layout_rfdetr_seg_2xl/dettrace.rs`,
hachage blake3 bit-exact des tensors intermédiaires du forward — preprocess, embeddings,
chaque couche DINO (norm1/attention/residual1/mlp/residual2), couches decoder, sorties).
Résultats : (1) session dettrace (27 rec/forward) — pages 2-3 divergent entre runs dès les
couches DINO 4-7, **toutes les entrées de la couche divergente identiques** (preprocess,
embeddings, couches 0-6) ; (2) session dettrace2 (87 rec) — page 3 seule divergente,
couche DINO 7, entre `dino_residual1` et `dino_mlp` (= norm2 → fc1 → GELU → fc2, GEMM
cuBLAS F32, TF32 off, fp32 via `--torch-fp32`) ; (3) sessions dettrace3/dettrace4 (9 runs,
135 rec avec sondes fc1/GELU/fc2) — **0 divergence**. Lecture : le postprocess (topk+seuil,
pas de NMS), les embeddings, la SDPA et le decoder ne divergent jamais — seul le chemin
GEMM du backbone est pris en flagrant délit, et les copies D2H par étage (sondes fines)
font disparaître la variance : signature d'une sélection d'algorithme cuBLASLt dépendante
du process/de l'état machine (workspace, heuristiques), pas d'une course dans un kernel.
Les SHA de run divergents tombent toujours sur les mêmes pages (2-3, jamais la page 1 —
premier forward du process) et se groupent par session comme DETHASH. Piste de fix :
épingler l'algorithme cuBLASLt (workspace fixe, `CUBLASLT_WORKSPACE_SIZE`, ou contournement
au niveau des shapes de batch) — mais l'intermittence impose de capturer une session
divergente avec les sondes fines avant d'affirmer l'op exacte. Les modèles Torch partagés
par le même process (OCR PaddleOCR-VL quantisé, LaMa) peuvent subir la même sélection —
cohérent avec l'OCRDB/OCRDBG des sessions antérieures.

**Session-grouping testé et RENVERSÉ (2026-09-29, 6 runs GPU process séparés + pauses
90 s, `target/llm-ab/session/`)** : l'hypothèse d'une variance groupée par session GPU
(suggérée par r02=r03 dans la passe gpuhash) ne tient pas. Résultats : **DETHASH
identique 6/6** (la détection layout est parfaite cette session-là — 3 hash :
`6f4eaffc… 782c4248… 0581d78b…`) ; mais **PATCHHASH diverge dès l'étage OCR** pour les
pages 2-3 (idx 10 : 4 valeurs différentes en 6 runs, dont `8a468464…` ×2 ; idx 11 : 4
valeurs ; idx 12 : 6/6 différentes — les traductions), et les **prompts texte LLM des
pages 2-3 varient run-to-run** (`LLMDBG prompt` 8b510fdc/32ad58ec dans r01-r03 vs
fca3e8ae/95dc3083 dans r04, aada9a64/b1ddb572 dans r05, 0ae16fbf/6279586d dans r06 —
avec `history` (tokens) différents en conséquence, y compris `prompt_tokens` 1703 vs 1704
! 1). `first_token` stable 4/4 dans les 6 runs malgré ces prompts différents (le prompt
diffère mais l'argmax initial coïncide), et le flip-rate LLM reste 3,57/1000. Lecture
révisée : (1) la variance n'est PAS groupée par session process — les runs se ressemblent
par blocs contigus dans le temps (r01-r03 identiques entre eux, r04-r06 tous différents)
mais 6 process séparés à 90 s d'intervalle divergent quand même → la « session » qui
groupe est temporelle (état du driver/machine, proche du contrôle cuDNN du 2026-09-28),
pas liée au process ; (2) **l'OCR (PaddleOCR-VL) est un deuxième producteur de variance,
indépendant de la détection** — cette session-là la détection était stable mais l'OCR a
varié sur les pages 2-3, changeant le texte (donc prompts/tokens LLM) ; la page 1 reste
stable dans les deux étages, comme dans toutes les sessions — probablement car son crop
d'image est différent (plus grand ?) ou son premier forward épinglé ; (3) le LLM reste un
amplificateur fidèle d'entrées déjà variables, jamais l'initiateur. Le pattern « pages
stables = page 1 uniquement » + variabilité OCR/détection intermittente par fenêtre
temporelle reste cohérent avec un état GPU/driver qui dérive (température, pression
mémoire, résidence cuBLAS/cuDNN) et affecte les algos GEMM des deux modèles Torch.

**CUBLASLT_WORKSPACE_SIZE=4096 testé (2026-09-29, 6 runs GPU process séparés + pauses
90 s, `target/llm-ab/ltws/`) — AUCUN effet observable** : l'env var (en KiB, fixant le workspace
cuBLASLt que PyTorch alloue ; héritée par le process relancé — vérifié) ne stabilise ni
la détection (DETHASH : pages 2-3 instables 2/6 runs, r02/r03 hors bloc — la session de
référence sans la variable était 6/6 stable, mais l'intermittence par fenêtre temporelle
rend la comparaison A/B sur sessions uniques non concluante en termes de dégradation) ni
l'OCR (PATCHHASH : 6/6 groupes distincts, idx 10 : 5 valeurs ; traductions 6/6) ni les
prompts (pages 2-3 tous différents run-to-run). Le flip-rate LLM reste du même ordre.
Lecture : la taille du workspace ne contraint pas assez les heuristiques cuBLASLt —
l'algorithme par shape reste choisi par heuristique à l'init du stream/handle et varie
avec l'état machine (température/pressures), l'épinglage effectif exigerait d'intercepter
`cublasLtMatmulAlgoGetHeuristic` ou de fixer l'algo par configuration C — hors de portée
du knob env. Conclusion mise à jour : ni workspace ni CUBLAS_WORKSPACE_CONFIG ne suffisent
; les correctifs réalistes restent (a) figer par processus (1 run = 1 groupe déjà
observé), (b) post-vérifier au niveau texte (comparer sorties OCR/translation entre runs
et rejouer les pages divergentes), ou (c) porter les modèles Torch sensibles vers des
backends sans heuristique (DirectML/CPU pour l'OCR court, no-op pour la détection lourde).
La variance par fenêtre temporelle (blocs r01-r03 stables puis rupture) reste le pattern
dominant, indépendante du workspace.

**Instrumentation OCRTRACE des crops (2026-09-29, `KOHARU_OCRTRACE=1`, 2 runs sans cache
OCR, `target/llm-ab/ocrtrace/`) — les shapes ne sont PAS la cause ; la stabilité de la
page 1 reste inexpliquée mais n'est pas structurelle** : nouvelle sonde dans `ocr.rs`
(page UUID, taille source, et pour chaque crop : dims + aire) — les 25 crops (13/4/8 par
page) ont des **shapes et un ordre strictement identiques entre les deux runs** (l'ordre
des régions est déterministe), et les `LLMDBG shape` des appels OCR (llama.cpp/mtmd,
le préprocesseur Torch de `paddle_ocr_vl` n'est PAS utilisé par le batch — la sonde
`resize` y est restée muette) sont identiques 25/25, de même que les `first_token` OCR
0/25 divergents et les prompts/history 0/25. Pourtant : **l'OCR de la page 3 diverge en
cours de décodage** (texte différent en sortie, prompts de traduction des pages 2-3
différents en conséquence — 2 des 4 LLMDBG shape de traduction diffèrent de 1-2 tokens),
et c'est bien le même pattern que toutes les sessions : **les entrées des appels OCR sont
bit-identiques, seul le décodage OCR flippe** — le flip OCR est donc un événement
llama.cpp (mtmd ViT + décode), pas un artefact de crop/shape, et il s'additionne au flip
llm de traduction comme consommateur indépendant du même non-déterminisme. Sur la page 1
(13 crops) : shapes identiques ET texte stable — aucun trait structurel visible (ce n'est
ni le premier forward du modèle OCR — des crops de la page 1 sont déjà les premiers appels
partagés avec les pages suivantes — ni une shape particulière : les dims des crops de la
page 1 couvrent la même gamme que les pages 2-3). L'hypothèse "page 1 = premier forward
épinglé" est affaiblie : les appels OCR de la page 1 ne sont pas tous premiers du process
(la détection et les warmups passent avant), et la page 1 reste stable même dans les
sessions où la page 2 diverge dès son premier crop. Reste une piste : la page 1 est la
seule avec une résolution source différente (1261x1807 vs 1126x1600) — à tester en
déroulant un corpus avec pages 1 et 2 de même résolution. En attendant, la page 1 stable
ressemble à un hasard structurel du corpus (son contenu est plus facile à OCR, moins de
near-ties), pas à un mécanisme d'épinglage.

**Corpus permuté/rescalé (2026-09-29, `target/val-perm/`, 2 runs OCRTRACE,
`target/llm-ab/permut/`) — la stabilité de la page 1 n'est NI la position NI la résolution
NI le contenu** : corpus à 4 pages = ex-page2 en position 1 (1126x1600), ex-page3 en 2,
ex-page1 rescalée en 1126x1600 en 3, ex-page1 originale (1261x1807) en 4. Résultat :
**DETHASH diffère entre les 2 runs pour les 4 pages**, y compris la page en première
position (ex-page2 : `8f95fd10` vs `301e182d`) et l'originale 1261x1807 en position 4
(`5bb5e307` vs `d59144d6`). Les crops restent identiques intra-run (shapes/ordre).
Conclusions : (1) l'ancienne stabilité de la page 1 n'était ni structurelle (première
position, résolution, contenu) ni durable — c'était un échantillon favorables dans une
variance de fond qui touche potentiellement toutes les pages, avec des fenêtres où
certaines passent ; (2) le pattern « certaines pages stables, d'autres non » n'est pas
prévisible par les propriétés de la page — il est dominé par l'état temporel de la machine
au moment du forward, cohérent avec tout le reste (grouping par fenêtre, intermittence
des sessions, entrées bit-identiques mais décodage différent). La variance touche le
décode llama.cpp quel que soit le contenu : aucune propriété de la page ne protège.

**Sonde `LLMDBG logits0` (2026-09-29, blake3 des logits complets du prefill, 4 runs
`target/llm-ab/logits/`) — le PREFILL est déjà non déterministe, l'hypothèse « prefill
stable » est RENVERSÉE** : jusqu'ici `first_token` identique 40/40 était interprété comme
prefill déterministe — c'était un artefact d'argmax : les logits complets divergent à
tout appel. Traductions : `logits0` distinct 4/4 runs pour les 4 pages (top-1 token
identique `2717`, mais top logit qui varie en 4ᵉ-5ᵈ chiffre significatif :
2.7666/2.7715/2.7677/2.7706e1 — jitter relatif ~1e-3) ; OCR : 18/25 appels avec `logits0`
distinct (7 stables), 22 valeurs distinctes de `first_token` (des crops différents).
Conclusions : (1) la divergence n'est PAS spécifique au décodage incrémental — le prefill
ViT mtmd + GEMM batchés produit déjà des logits différents run-to-run ; le décodage ne
fait qu'accumuler ce bruit jusqu'aux near-ties (hotspots) ; (2) `first_token` restait
stable parce que l'écart top-1/top-2 est large devant le jitter — les flips de décodage
apparaissent quand la cascade OCR→prompt rapproche les logits ; (3) le jitter est
déterministe-par-process ? Non : 4 process distincts donnent 4 logits distincts, donc le
bruit varie par process ET par appel (7 OCR stables = crops dont le jitter reste sous la
résolution blake3 ? improbable — plutôt crops à logits plus robustes). Piste de fix
devenue prioritaire : le prefill llama.cpp (batched GEMM cuBLAS dans ggml-cuda) est le
point d'entrée du bruit — côté options : `GGML_CUDA_FORCE_MMQ` ne change rien (testé),
le JIT cuBLASLt heuristique est suspecté ; instrumenter ggml côté splits/batch ou tester
un runtime avec cuBLAS workspace épinglé côté ggml (GGML_CUDA_ALLOC/scratch) ; à défaut,
la sonde OCRDBG/LLMDBG + post-vérification texte reste la protection pragmatique.

**Post-vérification `--verify` livrée (2026-09-29)** : `koharu-batch --verify` rejoue
le pipeline complet (détection, OCR sans cache, inpainting, traduction) sur une session
fraîche chargée depuis les mêmes fichiers, redirigé vers `<output>.verify`, et compare
les textes OCR (`SourceText`) et traductions (`Translation`) page par page par
(chapitre, label de page) — rapport stderr `verify: N page(s) replayed, M with drift` +
liste des pages instables, exit 0. Validé en réel : 3 pages rejouées, page1/page3
translation=DIFFERS, page2 stable. Pièges résolus pendant l'implémentation : le replay
réutilise la `resolved` de la première passe (un budget VRAM re-queryé voit ~0 car le
LLM est résident) ; `pipeline.unload_models()` (nouveau, décharge tous les stages du
runner courant via `Translator::unload` qui drop l'Arc) est appelé avant le replay sinon
l'OCR rejoué meurt sur `0 MiB free` ; le cache OCR est désactivé côté replay (sinon il
rejouerait les lectures de la passe 1 et masquerait le drift OCR) ; les pages sont
collectées depuis `snapshot.pages()` et non `chapter.loaded` (drainé par translation) ;
`prepare_chapters` du replay force `overwrite=true` (sinon un `.verify` préexistant
donne « nothing to do »). La comparaison OCR ne détecte du drift que si le cache OCR est
absent au premier passage — avec cache, la passe 1 rejoue les lectures stockées et seul
le drift de traduction est visible (le cas production : cache actif).

**`--verify-passes N` livré (2026-09-29)** : `--verify` vote désormais sur N passes
(production incluse, N≥2 forcé ; défaut 2 = comportement précédent). Refactor :
`replay_pass` (nouvelle fn run.rs) exécute un replay complet avec les mêmes pièges
résolus (no_ocr_cache/overwrite/output `.verify`/metadata), appelée N-1 fois avec le
MÊME pipeline/renderer/rasterizer construits une fois dans `verify_replay` — les stages
restent résidents entre passes de replay (pas de rechargement modèle, re-préparation
des chapters seulement ; passe 2+ ~1 min vs ~2 min). Collection : `pass_readings`
empreinte par page (clé label chapitre+page, blake3 tronqué 16 hex des sets triés OCR et
traduction — comparaison insensible à l'ordre des entities) ; une passe qui rate une
page compte comme lecture distincte "absent" (drift). Vote : comptes par lecture
distincte par axe ; rapport stderr : `N page(s) voted over P passes, S stable, M with
drift` + par page instable `ocr D distinct (fréqs) translation D distinct (fréqs) ->
P(drift)~K/P` où K = passes − min(lecture majoritaire OCR, traduction) (probabilité que
la page diffère de sa lecture majoritaire sur l'un des deux axes, production incluse).
Validé réel N=3 val-in : `3 page(s) voted over 3 passes, 1 stable, 2 with drift` ;
page1/page3 : translation 3 distinct (1,1,1) → P(drift)~2/3 (page1 drift 3/3 passes,
≠ 2/3 des runs 2-passes antérieurs, échantillon N=3 plus fin) ; page2 : ocr (3)
translation (3) stable ; OCR set stable partout (cache actif en passe 1, attendu). Logs
`target/llm-ab/verify/n03.log`, script `n03-run.sh`. Piège du build/test : ne jamais
lancer nohup avec `& disown` direct au premier plan du tool (timeout tool kill le pipe
mais nohup survit — utiliser `(nohup … &)`) ; le terminal tool timeout max 600 s (un sleep
>600 dans un run réel de ~5 min typo-pas-problème). Usage : `--verify --verify-passes 3`
(production+N replays). Les scripts de test sont dans `target/llm-ab/verify/` (`n02-run.sh`,
`n03-run.sh`).

Régression N=2 (chemin par défaut `--verify`) : `3 page(s) voted over 2 passes, 0 stable, 3 with
drift` (log `target/llm-ab/verify/n02.log`, script `n02-run.sh`) — toutes les pages en
translation `(1,1) → P(drift)~1/2`, OCR set stable partout `(2)` ; 3/3 pages instables vs `2
with drift` du run N=2 précédent (le rapport 2 passes avec vote affiche désormais 0 stable car
les 2 passes donées diffèrent, vs le format bool DEERS/stables précédent). Compatibilité : le
chemin exact de `--verify` seul (N=2) donne un rapport 2 passes avec vote identique en
signification au format Bool antérieur, une page instable = 2 lectures distinctes = `(1,1)`
`P(drift)~1/2`. Différence clé vs run 2-passes précédent : N=2 sous-échantillonne — page2
(stable sur N=3) est instable sur N=2, ce qui justifie le mode N passes pour estimer P(drift)
plutôt que de conclure binairement. Note sur le vote 2 passes : P(drift)~1/2 est un
estimateur biaisé (2 passes, la fréquence de lecture distincte ne peut valoir que 0,½ ou 1) ;
le N≥3 est le régime interprétable.

**⚠ Contrôle final 2026-09-28 soir : le fix cuDNN N'EST PAS inconditionnel.** Trois runs
consécutifs (A/B/C, ~1 min d'intervalle) : les 3 détections de A et B diffèrent toutes
(entries identiques, DETHASH vides dans l'extraction, divergence dès detection) ; la passe
C rejouée 2 min plus tard retombe sur les hash de A pour 2 pages sur 3 et reste divergente
pour la 3ᵉ. Le GPU était sous charge croissante d'autres processus (Steam webhelper,
LGHUB, terminal — nvidia-smi liste des compute apps au moment du test, absents lors de la
validation initiale de 15 h). Lecture : la sélection d'engine déterministe cuDNN tient en
session GPU calme mais **sous contention, cuBLAS/cuDNN résident dans le driver changent de
chemin** — l'atomique n'était pas seule en cause, ou le knob `setDeterministicCuDNN` ne
serre pas le même axe. À réinstruire avec : la machine au calme (fenêtre 19:20 = session
GPU chargée), un run long pour saturer, et la sonde DETHASH par run. Ne pas présenter le
fix comme acquis tant que ce cas n'est pas compris. Deux pièges de mesure : `chapter_context` propage les traductions déjà dérivées
dans les prompts des pages suivantes (contamination en cascade) ; et l'ordre d'exécution des
pages par étage GPU peut varier entre runs (comparer par `entries=`/prompt hash, pas par
position). Correctif à chercher côté upstream : rebuild du package llama avec kernels
déterministes ou release ggml corrigeant l'op atomique en cause.

## Pièges

- **Le fmt du Lint CI est propre depuis `4466b975`** (vérifié 2026-09-25 :
  `cargo fmt --all -- --check` → 0 diff ; la vieille note « 42 diffs préexistants » est
  obsolète). Après édition Rust : `cargo fmt --all` (et pas seulement `-p` sur un crate).
- **Toujours tester avec `--no-calibration`** ; ne jamais toucher
  `C:\Users\endymion\.koharu\vram-calibration.toml`.
- **`cargo test --workspace --tests` dépasse 10 min à froid** : le run tool timeout à 600 s —
  relancer (cargo reprend) ou lancer par crate. Build release complet : ~6-10 min.
- **Chemins Windows pour les runs réels** : passer `--store D:/koharu/store` (pas `/d/...`) au
  binaire release.
- **Rebuild du binaire de banc** : `cargo build --release` ne build PAS `koharu-batch` (cible
  hors défaut) — viser `cargo build --release --locked -p koharu-pipeline --bin koharu-batch`,
  après `cargo clean -p koharu-pipeline --release` (un `clean -p` sans `--release` ne nettoie
  que le debug et laisse le binaire périmé ; des hash md5 identiques trahissent un binaire
  hérité). Ne pas builder les deux checkouts dans un target partagé : les artefacts du
  worktree écrasent ceux de main — le worktree avec son propre `target/` est isolé.
  `koharu.exe` réclame `libcef.dll` à côté (exit 53 = STATUS_DLL_NOT_FOUND), pas
  `koharu-batch.exe`.
- **cargo n'est pas sur le PATH du shell Codebuff** : le binaire vit dans
  `~/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/` — préfixer le PATH pour toute
  commande cargo. CARGO_HOME reste le défaut `C:\Users\endymion\.cargo` (sans `bin/`).
- **« Exit code 1 » PowerShell après `git push`** = artefact stderr (git écrit la progression sur
  stderr), pas un échec : la ligne `... main -> main` confirme la réussite.
- **`koharu/` est son propre dépôt git** (le dépôt `D:\Codex` le voit comme non suivi) :
  committer depuis `koharu/`, pas à la racine. Ne jamais pousser sans demande (les workflows
  `lint`/`test` se déclenchent sur push `main`).
- **TypeScript 7.0.2 est épinglé (`package.json`) et strict par défaut** :
  `bunx tsc -p scripts/tsconfig.json` (étape CI *Typecheck scripts*) sort
  TS7006/TS18048/TS2769 sur du code `any` toléré sous TS5 — corrigé le 2026-10-02 dans
  `scripts/test-prune-step.ts` (document YAML typé, `step.run` défendu, `RUNNER_TEMP`/`TEMP`
  gardé). Ce typecheck échoue aussi si on le lance depuis `D:\codex\…` en TS1149 (casse
  `D:\codex` vs `D:\Codex` héritée des liens workspace) : artefact local, relancer depuis
  `D:\Codex\…` — CI (Linux, chemin unique) ne le voit pas.
- **`gh` pointe par défaut sur l'UPSTREAM `koharu-rs/koharu`** (config du poste) :
  `gh run list` affiche alors les runs d'un AUTRE dépôt — ceux du fork n'y figurent
  jamais, et `gh run view <id>` répond 404. Passer `-R Endymi0n74/Koharu` ou l'API
  explicite `gh api repos/Endymi0n74/Koharu/actions/runs?head_sha=<sha>` (piège constaté
  2026-10-02 : veilleur de CI aveugle pendant 55 min). Autre subtilité : l'option `--jq`
  de `gh` perd les guillemets imbriqués sous PowerShell 5.1 — parser en PowerShell.
- **Vert sous Windows ≠ vert au Lint CI (Ubuntu)** : un import utilisé uniquement dans du
  code `#[cfg(windows)]` est « unused » sous Linux (cas `bail` dans
  `koharu-ml/src/determinism.rs`, fix `e5624756`). Les branches `not(windows)` et les
  `*.rs` de plateforme (`resources/linux.rs`) ne sont jamais lints en local : face à un
  `unused import`/`dead_code` au Lint, chercher un cfg de plateforme d'abord.
- **Smoke test** (exit attendu 0) :
  ```powershell
  .\target\release\koharu-batch.exe --input "crates\koharu-ml\benches\fixtures\object_detection" `
    --output "$env:TEMP\koharu-smoke\out" --llm gemma4-e4b-it `
    --report "$env:TEMP\koharu-smoke\report" --overwrite --no-calibration
  ```

## CI (état local vérifié 2026-09-25 ; passage vert complet 2026-10-02 sur `e5624756`)

| Workflow | Déclencheur | État |
|---|---|---|
| `build.yml` / `test.yml` | push `main` | verts (2026-10-02) |
| `koharu-batch.yml` | `main` + tag | vert (2026-10-02) : smokes `models` + `prune --delete` sur les sous-commandes **validés en CI réel**, + smoke `bun scripts/bench-cross.ts --plan` |
| `lint.yml` | push `main` | vert (2026-10-02) après deux fixes : typecheck scripts sous TypeScript 7 (`1158e6e3`) et clippy Linux `determinism.rs` (`e5624756`) + guard `bun scripts/check-path-portability.ts` (bannit les littéraux `Path::new(r"C:\...")` — piège du fix `3cd290b9`) |
| `release.yml` | tag `v*` | Windows / Ubuntu / ARM ✅ ; macOS désactivé |

**Release = tag `v*` déclenche `release.yml`.** Pousser sur `main` ne publie rien.
Commandes CI = vérifier localement : `cargo fmt --all -- --check`, `cargo check`,
`cargo clippy -- -D warnings`, `cargo test --workspace --tests`,
`bun run --filter @koharu/app lint`, `bun run --filter '@koharu/*' typecheck`,
`bun run --filter @koharu/app test` (106 tests).

## À faire plus tard (choix ouverts)

- Résidu translation : **cause racine identifiée le 2026-09-29 — la variance naît dans la
  détection layout RFDetR-SEG (Torch), pas dans les kernels llama** (cascade DETHASH →
  PATCHHASH → OCR → traduction ; CPU `--no-vision` bit-stable). Correctif attendu côté
  détection : seed/mode eval/NMS déterministe du modèle Torch, ou fix cuDNN étendu à RFDetR.
  Le patch du runtime llama (#16016) est inutile ici (voir section Non-déterminisme batch :
  MMQ et FA=OFF falsifiés, contention falsifiée, CPU aussi flippe avec vision).
- Sort des sondes `DETHASH` (`stages/detection.rs`) et `LLMDBG`
  (`koharu_ml::llm::model`) : **décidé le 2026-10-02 — GARDER**, jusqu'à ce que le correctif
  détection soit prouvé inconditionnel (le contrôle du 2026-09-28 l'a montré non acquis sous
  contention GPU) : elles restent les instruments du chantier déterminisme et des scripts
  `scripts/flip-study*.py`. Retirer seulement une fois la stabilité acquise (critère : N runs
  A/B `KOHARU_PATCH_HASH` sans divergence DETHASH), en purgeant aussi `flip-study*.py`.
  Durci au passage : `debug_enabled()` de LLMDBG échantillonne `KOHARU_LLM_DEBUG` une seule
  fois par process (fini une lecture env par token décodé).
- Réactiver le job macOS : **préparé le 2026-10-02, bloqué sur les secrets Apple** — vérifié
  par API : seuls `TAURI_SIGNING_PRIVATE_KEY{,_PASSWORD}` existent. Créer les 5 secrets
  listés dans le commentaire de `.github/workflows/release.yml` (`BUILD_CERTIFICATE_BASE64`,
  `KEYCHAIN_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM`) puis décommenter l'entrée
  `macos-latest` de la matrice ; les étapes macOS sont déjà écrites et `if:`-gardées.
- Mode dossier dans `koharu-app` (piloter un lot en GUI).
