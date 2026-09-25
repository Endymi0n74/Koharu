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
  : `cli.rs`, `run.rs`) — **106 tests lib** exécutés par CI (`cargo test --tests` n'exécute pas
  les tests d'une cible binaire). Exécution **phase-major** ; exit 0 = toutes les pages réussies.
- **Robustesse batch** : `--retries` (défaut 1) par stage, rapports réécrits à chaque phase/page
  (checkpoint), toutes les erreurs de finalisation restent des échecs page, reprise CBZ avec
  report des entrées déjà traduites (`carried`) et extension de l'entrée lue dans les octets
  réels (fin du bug JPEG-nommé-PNG), `finish()` avant rename (Windows).
- **Fonctionnalités batch** : `--pages` (plage 1-based), `--recursive`, `--quiet`, progres
  `[i/n] stage · Xs · ETA`, **mode volume** (dossier sans images au premier niveau → un chapitre
  par sous-dossier et par .cbz, sorties miroir sous `--output`, rapports par chapitre + résumé
  global, phase-major sur tout le volume), **`--json <PATH>`** (résumé machine : ok/compteurs/
  chapitres/pages/échecs, écrit aussi en dry-run).
- **Perf batch** : finalisation (encode/écriture/vignette) sur un **thread worker** qui chevauche
  la traduction suivante (le render reste sur le thread principal — il emprunte la session) ;
  archive CBZ d'entrée ouverte **une seule fois** (`cbz::ArchiveReader`) au lieu d'une fois par
  page. Mesures : voir *Mesures* plus bas.
- **App desktop** : locale **fr-FR** complète (434 clés, tests de parité sur les 10 locales) +
  `languages.*` ; **mode dossier** (« Open a folder… » importe un dossier comme projet via
  `ProjectLibrary::folder_project_name`) ; `JobGuard` (Drop retire stop+job, panique → Failed).
- **CI** : step *Typecheck UI* dans `lint.yml` + `typecheck` dans `@koharu/app`, smoke tests
  `--dry-run` **et mode volume** dans `koharu-batch.yml`.
- **Release** : `lto = "thin"` dans le profil release ; tag **`v0.83.5`** publié ; job **macOS
  désactivé** dans `release.yml` (secrets Apple absents) — réactiver quand
  `BUILD_CERTIFICATE_BASE64` / `KEYCHAIN_PASSWORD` seront configurés.

## Décisions tranchées (ne pas rouvrir sans re-mesurer sur la cible)

- **Règle du mode volume** : ≥1 image au premier niveau ⇒ un chapitre (sous-dossiers ignorés) ;
  `--recursive` ⇒ arbre entier aplati en un chapitre ; sinon sous-dossiers + .cbz = volume ;
  rien ⇒ refus avec message expliquant les deux cas. Imprimée par `--dry-run`. Une seule
  profondeur de sous-dossiers (un sous-dossier est listé récursivement, mais un volume niché
  `Vol/Ch/p.png` est un seul chapitre `Vol`).
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

**Conclusion honnête** : la variance de génération du LLM (±10 s sur des pages identiques)
noie le gain dans le bruit total ; décomposé, la finalisation (encode PNG ~0,2-0,3 s/page +
écriture + vignette) sort du chemin critique et le coût par page mesuré est cohérent, mais le
gain mur est < 1,5 s par chapitre de 6 pages sur cette petite charge. L'ouverture du CBZ une
seule fois ne se distingue pas du bruit ici (l'ancien coût était déjà < 0,1 s/page sur un
archive de 6 entrées) mais supprime un `ZipArchive::new` complet par page — à re-mesurer sur
un vrai tankōbon (50+ pages, pages plus grandes) avant d'en faire un argument. E2E volume réel
(3 chapitres, 4 pages) : 47 s, sorties + rapports par chapitre + JSON, exit 0.

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

- Re-mesurer le finalizer worker sur un vrai tankōbon (50+ pages, pages lourdes) — la charge
  de mesure actuelle (6 petites pages) ne montre pas le gain.
- Volume imbriqué (>1 niveau de sous-dossiers) si un cas réel en a besoin.
- Réactiver le job macOS si les secrets Apple sont configurés.
- Mode dossier dans `koharu-app` (piloter un lot en GUI).
