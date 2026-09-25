# Koharu — mémoire de projet

Fork de [koharu-rs/koharu](https://github.com/koharu-rs/koharu) (traducteur de manga ML, Rust).
Remotes : `origin` = https://github.com/Endymi0n74/Koharu (branche `main`), `upstream` = koharu-rs/koharu.

Machine de référence : **RTX 3070 8 Go (sm_86)**. Store : `%LOCALAPPDATA%\koharu\packages`
(`D:\koharu\store` pour les runs e2e réels : `--store D:/koharu/store`).

Règles durables : [`AGENTS.md`](AGENTS.md). Ici : état du projet, décisions tranchées, pièges.

## Livré (sessions récentes)

- **Prompt enrichi** (`crates/koharu-translator/src/prompt.rs`) : `source_guidance` (JA/RU) +
  `target_style` (FR) — seul levier retenu pour la qualité JA/RU→FR (échantillonnage et pivot EN
  écartés). 81/81 tests translator.
- **`koharu-batch`** : logique déplacée du binaire vers la lib (`crates/koharu-pipeline/src/batch/`
  : `cli.rs`, `run.rs`) — **110 tests lib** exécutés par CI (`cargo test --tests` n'exécute pas
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
CBZ → CBZ, `gemma4-e4b-it`, avant = HEAD `4466b975`, après = worker finalizer. Les deux runs :
exit 0, 50/50 pages traduites, archives de ~202 Mo valides.

| Run | avant | après |
|---|---|---|
| mur total | 1294 s | 1083 s (−211 s, −16,3 %) |
| Σ étapes (traduction + phases) | 1261,4 s | 1065,8 s (−195,6 s) |
| résidu `mur − Σétapes` | 32,6 s | 17,2 s (**−15,4 s**) |
| chaîne hors étapes par page (load+render+finalisation) | 0,39 s | 0,36 s |

- **Le −211 s brut n'est PAS le gain** : −195,6 s sont du **drift d'étapes** (GPU chaud +
  variance d'échantillonnage ; le run « après » a passé après le « avant »).
- **Gain structurel = −15,4 s** (résidu immunitaire à la génération) ≈ 49 × 0,31 s : la
  finalisation (~0,3 s/page : encode PNG du render 1662×2560 + écriture + vignette) sort du
  chemin critique. Le **render (~2,2 s/page, déduit du gap inline 2,56 s/page) reste
  sérialisé** dans les deux binaires (borrow session) — c'est le plafond annoncé.
- Ouverture CBZ 1× vs 50× : < 0,1 s sur 50 entrées — invisible, comme prévu.
- Caveat d'ordre : « avant » exécuté en premier (GPU froid) — le résidu peut être surestimé
  de ~1-3 s par le warm-up du render. Un banc croisé (ordre inversé) le neutraliserait.

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
- **« Exit code 1 » PowerShell après `git push`** = artefact stderr (git écrit la progression sur
  stderr), pas un échec : la ligne `... main -> main` confirme la réussite.
- **`koharu/` est son propre dépôt git** (le dépôt `D:\Codex` le voit comme non suivi) :
  committer depuis `koharu/`, pas à la racine. Ne jamais pousser sans demande (les workflows
  `lint`/`test` se déclenchent sur push `main`).
- **Smoke test** (exit attendu 0) :
  ```powershell
  .\target\release\koharu-batch.exe --input "crates\koharu-ml\benches\fixtures\object_detection" `
    --output "$env:TEMP\koharu-smoke\out" --llm gemma4-e4b-it `
    --report "$env:TEMP\koharu-smoke\report" --overwrite --no-calibration
  ```

## CI (état local vérifié 2026-09-25)

| Workflow | Déclencheur | État |
|---|---|---|
| `build.yml` / `test.yml` | push `main` | verts |
| `koharu-batch.yml` | `main` + tag | vert (smoke dry-run + volume ajoutés) |
| `lint.yml` | push `main` | **vert localement** (fmt/check/clippy `-D warnings`/UI lint/typecheck tous OK) |
| `release.yml` | tag `v*` | Windows / Ubuntu / ARM ✅ ; macOS désactivé |

**Release = tag `v*` déclenche `release.yml`.** Pousser sur `main` ne publie rien.
Commandes CI = vérifier localement : `cargo fmt --all -- --check`, `cargo check`,
`cargo clippy -- -D warnings`, `cargo test --workspace --tests`,
`bun run --filter @koharu/app lint`, `bun run --filter '@koharu/*' typecheck`,
`bun run --filter @koharu/app test` (106 tests).

## À faire plus tard (choix ouverts)

- Banc croisé (ordre avant/après inversé) pour neutraliser le warm-up GPU du résidu du
  banc `real50` — si un argument public doit reposer sur les 15 s mesurées.
- Réactiver le job macOS si les secrets Apple sont configurés.
- Mode dossier dans `koharu-app` (piloter un lot en GUI).
