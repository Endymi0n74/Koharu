## Bundle installé déplacé hors du dépôt (25/11)

L'installation qui vivait à la racine du dépôt (`D:\Codex\koharu`) est
maintenant dans `D:\Codex\KoharuBatch` : `koharu.exe`, `koharu-batch.exe`,
`uninstall.exe`, le runtime CEF complet (`libcef.dll`, `.pak`, `icudtl.dat`,
`chrome_elf.dll`, `d3dcompiler_47.dll`, `dxcompiler.dll`, `dxil.dll`,
`v8_context_snapshot.bin`, `vk_swiftshader*`, `vulkan-1.dll`, `bootstrap*.exe`,
`CREDITS.html`) et `locales\` (220 fichiers). Le dossier ne contient plus aucun
fichier de l'application ; `target\` et `dist\` sont inchangés.

Suivi mis à jour :

- raccourcis `%APPDATA%\Microsoft\Windows\Start Menu\Programs\koharu.lnk` et
  `%USERPROFILE%\Desktop\koharu.lnk` : cible et dossier de travail
  `D:\Codex\KoharuBatch` ;
- clé `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\koharu` :
  `InstallLocation`, `UninstallString` et `DisplayIcon` repointés ;
  `uninstall.exe` ne contient aucun chemin en dur (non exécuté) ;
- le bloc « Application Koharu installee dans le dossier du depot » a été
  retiré de `.git/info/exclude`, et `git status` reste propre sans lui.

Vérifié : `koharu.exe` lancé depuis `D:\Codex\KoharuBatch` (chemin confirmé par
`Get-Process`), fenêtre « KoharuBatch » répondante et cinq processus CEF, avec
le runtime CEF présent à côté du binaire. Les avertissements `list_models`
(Ollama/LM Studio absents) sont normaux.

## Vague 1 des caches de développement (25/11)

Les sept caches de la vague 1 sont sur `D:\Codex\dev`. C: est passé de 28 à
41 Go libres (223 → 211 Go utilisés).

| Élément | Taille | Destination | Méthode |
|---|---:|---|---|
| `.cargo` | 4,11 Go | `D:\Codex\dev\.cargo` | Jonction `C:\Users\endymion\.cargo` |
| cache pip | 3,16 Go | `D:\Codex\dev\pip-cache` | `PIP_CACHE_DIR` (variable utilisateur) |
| `.codex` | 1,73 Go | `D:\Codex\dev\.codex` | Jonction ; `current` recréée vers `releases\0.169.0…` ; sockets exclus |
| npm global | 1,52 Go | `D:\Codex\dev\npm` | Jonction `%APPDATA%\npm` |
| `.gradle` | 0,88 Go | `D:\Codex\dev\gradle` | `GRADLE_USER_HOME` (variable utilisateur) |
| OpenCode | 0,82 Go | `D:\Codex\dev\opencode` | Jonction `~\.local\share\opencode` |
| `.rustup` | 0,56 Go | `D:\Codex\dev\.rustup` | Jonction `C:\Users\endymion\.rustup` |

Total déplacé : environ 12,8 Go.

Deux incidents réparés pendant la reprise :

- robocopy avec `/SL` a échoué sur 29 liens symboliques (LICENSE, README,
  examples) car Windows exige un privilège pour les recréer. Les fichiers
  réguliers étaient déjà sur D ; les liens ont été remplacés par des copies de
  leurs cibles, sur D.
- Les proxys `cargo.exe`, `rustc.exe`, `rustup.exe`, etc. de `.cargo\bin`
  n'avaient pas suivi ; ils ont été recréés en hardlinks de
  `.rustup\bin\rustup.exe` (14 liens). `cargo-tauri.exe` et `wasm-pack.exe`
  sont conservés.

Le service OpenCode (`opencode-cli`, `opencode-pty`) a été arrêté pour libérer
sa base SQLite ; il n'a pas été relancé. Codex n'était pas lancé : ses sockets
seront recréés à la prochaine ouverture.

Vérifié après déplacement :

- `cargo check -p koharu-batch` : exit 0 en 1 min 50 s.
- `bun run --filter @koharu/app build` : exit 0 (TypeScript et pages statiques).
- `koharu.exe` lancé depuis `D:\Codex\koharu` : fenêtre « KoharuBatch »
  répondante, puis fermée.
- `cargo 1.97.1`, `rustc 1.97.1`, `rustup 1.29.1`, `bun 1.3.14`, `npm 11.19.0`.
- `pip cache dir` répond `d:\codex\dev\pip-cache` ; `GRADLE_USER_HOME` et
  `PIP_CACHE_DIR` sont bien dans l'environnement utilisateur.
- Jonctions résolues : `.cargo`, `.rustup`, `.codex`, `%APPDATA%\npm`,
  `~/.local/share/opencode`.

Logs : `target/move-caches.log` (premier essai), `target/move-caches-2.log`,
`target/cargo-check.log`, `target/bun-build.log`.

## Rien sur C: (25/11)

Tout ce qui restait de KoharuBatch sous `C:\` a été déplacé vers `D:\Codex` :

- `%LOCALAPPDATA%\Koharu` (ancienne installation per-user, 1,9 Go, dont
  `packages\cuda` 662 Mo et `packages\torch` 209 Mo obsolètes) : binaires,
  runtime CEF et paquets supprimés ; les caches `cef`, `cef-debug` et `logs`
  sont conservés dans `D:\Codex\koharu-data\AppData\Local\Koharu`. Projets
  vides, donc rien d'autre à préserver.
- `~/.koharu` (config, backups, `vram-calibration.toml`) :
  `D:\Codex\koharu-data\.koharu`.
- `%APPDATA%\Koharu` (état de fenêtre) :
  `D:\Codex\koharu-data\AppData\Roaming\Koharu`.
- `%LOCALAPPDATA%\tauri-cef` (cache de build CEF, 594 Mo) :
  `D:\Codex\tauri-cef`.
- `%LOCALAPPDATA%\Temp\koharu-0.88.3-updater-*` et `build_koharu.log` :
  supprimés.

Les trois emplacements de données de l'app sont remplacés par des jonctions
NTFS de 0 octet, pour que l'app installée dans `D:\Codex\koharu` continue de
les trouver sans jamais écrire sur C: :

    C:\Users\endymion\.koharu
        -> D:\Codex\koharu-data\.koharu
    C:\Users\endymion\AppData\Local\Koharu
        -> D:\Codex\koharu-data\AppData\Local\Koharu
    C:\Users\endymion\AppData\Roaming\Koharu
        -> D:\Codex\koharu-data\AppData\Roaming\Koharu
    C:\Users\endymion\AppData\Local\tauri-cef
        -> D:\Codex\tauri-cef

Le store (75 Go), les projets et les binaires sont déjà dans
`D:\Codex\koharu` ; les raccourcis du menu Démarrer et du bureau pointent
déjà sur `D:\Codex\koharu\koharu.exe`. C: est passé de 26 à 28 Go libres.

## Version unique 0.93.0

La version du produit vient uniquement de `[workspace.package] version` dans
`Cargo.toml`. release-please la lit et la réécrit par
`cargo set-version --workspace` ; son manifeste
`.github/release-please/manifest.json` est aligné sur 0.93.0. Le build Tauri
n'a plus d'override de version (`crates/koharu/tauri.release.conf.json` ne fixe
plus que le bundle) : il prend la version du paquet `koharu`, qui hérite du
workspace. Le suffixe `-fork` disparaît des versions ; le dépôt et le nom
affiché restent KoharuBatch.

Vérifié : `cargo metadata` donne 0.93.0 pour les crates du workspace,
`Cargo.lock` ne contient plus `0.93.0-fork`, et le manifeste release-please
vaut 0.93.0.

Installeur reconstruit sans override le 25/11 avec
`cargo tauri build --config crates/koharu/tauri.release.conf.json` :
`koharu_0.93.0_x64-setup.exe` (198 348 423 o), FileVersion et ProductVersion
0.93.0 ; `koharu-batch --version` répond `koharu-batch 0.93.0`. Les copies
installées `D:\Codex\koharu` (cible du raccourci), `%LOCALAPPDATA%\Koharu` et
`%LOCALAPPDATA%\koharu-clean-093` ont été remplacées par ces binaires. Le ZIP
portable n'a pas été reconstruit.

## Renommage affiché et montée 0.92.0

### Décision de renommage (tranchée)

Seul le nom affiché devient KoharuBatch :

- titre de fenêtre (`crates/koharu/tauri.conf.json`),
- métadonnée et textes de l'app (`packages/koharu/app/layout.tsx`,
  `packages/koharu/components/app/{AboutDialog,StartupView,TitleBar}.tsx`),
- README.

Reste en `koharu` : le binaire de l'app et donc `productName`, l'identifiant
Tauri `Koharu`, le dépôt, les crates, les modèles HuggingFace, les variables
`KOHARU_*` et les références `koharu/…` du catalogue. Le nom du CLI reste
`koharu-batch`.

### Montée 0.92.0

`[workspace.package] version` de `Cargo.toml` et l'objet `version` du manifeste
de release-please sont maintenus à 0.92.0. Le tag `v0.92.0` est une étiquette
légère posée à la main sur le commit de release du port, puis poussée sur
`origin`.

### Suivi des fichiers touchés pour le renommage affiché

- `crates/koharu/tauri.conf.json` : `title` `Koharu` → `KoharuBatch`.
- `packages/koharu/app/layout.tsx` : `title` `Koharu` → `KoharuBatch`.
- `packages/koharu/components/app/AboutDialog.tsx` : `DialogTitle` `Koharu` → `KoharuBatch`.
- `packages/koharu/components/app/StartupView.tsx` : texte du titre `Koharu` → `KoharuBatch`.
- `packages/koharu/components/app/TitleBar.tsx` : `alt` et le texte non-formulaire `Koharu` → `KoharuBatch`.
- `README.md` : titre, description, menus et références du produit.

## prune — deux racines de modèles en journaux

`koharu-batch prune` classe les deux racines du store, pas seulement celle que
0.92 écrit :

- `store/huggingface` (ce que 0.92 télécharge) — gardé au **commit**, comme avant ;
- `store/hugging-face` (ce que le fork téléchargeait avant le renommage upstream
  de Hugging Face) — gardé au **dépôt** : un dépôt qu'un catalogue pointe encore
  reste entier, quel que soit le révision que le fork y a laissée.

Ce n'est pas une compatibilité. `prune` lit seulement les pins 0.92 et leurs
dépôts/downloads. Le fork conservé sert à ne pas supprimer un dépôt encore
proposé ; on ne propose jamais un dépôt `hugging-face` en l'état.

Rapport : deux blocs, `périmés` puis `réutilisables`, avec leur taille ; rien
d'invisible dans les ~50 GiB de `hugging-face` qui correspond à un pin 0.92.

### Cas `baberu-ocr`

Pin 0.92 : `genshiai-daichi--baberu-ocr` à `a0e3d4b1…` (`huggingface`). Le store
contient `genshiai-daichi--baberu-ocr` à `d9cc131…` sous `hugging-face`. Même
dépôt, autre révision : le dépôt est gardé au nom du pin, mais le télécharge
fork ne correspond pas au commit 0.92 — adoption = re-télécharge d'environ 1 GiB,
choix conservateur assumé.

Décision committée sur la branche `port/upstream-0.92`. Le rapport affiche les
périmés puis les réutilisables séparément ; les réutilisables sont dérivés des
deux racines en même temps que les pins, sans récrire de catalogue.

## Version 0.93.0-fork

`[workspace.package] version` passe à `0.93.0-fork` ; le manifeste
release-please reste à 0.92.0, comme pour la montée précédente : il ne sert pas
à l'affichage et le tag se pose à la main.

Changements de comportement : le dialogue « Batch translate a folder »
affiche la progression du lot ; avant d'écraser un `project.khr`, `start_batch`
en garde une copie (voir « Garde-fous avant écrasement »). `koharu-batch run`
imprime le total (`N page stages to run`) avant les lignes
`<stage> done on <page>`, et `start_batch` les remonte au dialogue par un
`Channel`.

Distribution : `KoharuBatch-0.93.0-fork-windows-x64-portable.zip` (271 Mo,
603 Mo décompressés), assemblée depuis `target\release` avec le runtime CEF
156 et ses 220 locales ; `SHA256SUMS` joint. Vérifiée en lançant l'exécutable
extrait : fenêtre « KoharuBatch », version 0.93.0-fork.

## Garde-fous avant écrasement (0.93.0)

Quand `output\project.khr` existe, `start_batch` le copie en
`project.khr.bak` et copie `project.khr-wal` en `project.khr-wal.bak` avant
d'ajouter `--overwrite` ; un échec de copie arrête le lancement. Le dialogue
refuse le même dossier en entrée et en sortie (`batch.sameFolder`, traduit dans
les 12 locales), désactive Démarrer tant que les deux chemins sont égaux, et
efface une sortie identique à l'entrée à la réouverture.

Vérifié le 25/11 : test Vitest ciblé (4/4) et `cargo check` ; sur le build
debug, le même dossier injecté laisse la sortie vide et Démarrer désactivé ;
un lot relancé dans `test_output_093_backup` produit `project.khr.bak` avec le
SHA-256 du projet d'avant le run (`9e16ac…`) et `project.khr-wal.bak`, puis le
toast « Lot terminé ». Installeur et ZIP reconstruits ; l'installation a été
mise à jour dans `%LOCALAPPDATA%\koharu-clean-093` (le `/D=` est ignoré quand
une installation existe), fenêtre « KoharuBatch » et CLI `0.93.0-fork`.

## Pages du lot rendues (0.93.0)

`koharu-batch run` écrit `pageN.png` en rendant chaque page par le renderer
partagé (`koharu-renderer`), comme l'export PNG de l'app, au lieu d'un canevas
vide de la taille de la page. Vérifié par
`render_page_writes_each_page_at_its_own_size` : deux pages de tailles
différentes (40×30 et 20×60), la première avec un aplat rouge, sont rendues et
relues depuis leurs PNG sur GPU.

Installeur reconstruit (198 381 686 o) et ZIP portable (270 997 060 o,
242 fichiers, SHA-256 `342e8ec6…`). Le raccourci `koharu.lnk` vise
`D:\Codex\koharu\koharu.exe` : le `koharu-batch.exe` de cette installation et
celui de `%LOCALAPPDATA%\Koharu` et `%LOCALAPPDATA%\koharu-clean-093` ont été
remplacés par le binaire reconstruit (`a553545c…`).

## Renommage complet Koharu -> KoharuBatch (0.92.0-fork)

Le renommage affiché de 0.92.0 reste le même, et le dépôt GitHub suit :
`Endymi0n74/Koharu` devient `Endymi0n74/KoharuBatch`, d'où le remote `origin`
mis à jour (`https://github.com/Endymi0n74/KoharuBatch.git`). `upstream` ne
bouge pas.

Renommé en `KoharuBatch` : titre de fenêtre (Windows, macOS, Linux), README,
AGENTS.md, CONTRIBUTING.md, README des crates, textes des 12 locales,
commentaires et messages de log, description des capacités Tauri, modèles
d'issue, `cliff.toml` (dépôt du fork), endpoint de mise à jour.

Restent en `koharu`, volontairement :

- les exécutables `koharu.exe` et `koharu-batch.exe` (paquets, bins,
  `productName` implicite) ;
- l'identifiant Tauri `Koharu` : il calcule les dossiers de données et de
  configuration de l'utilisateur (`AppData\Roaming\Koharu`, `~/.koharu`), donc
  le changer déplacerait les données existantes ;
- les crates, chemins, types (`KoharuStore`, `KoharuApp`), variables
  `KOHARU_*`, le modèle « Koharu Layout HighRes » et son dépôt Hugging Face ;
- les URL d'attribution upstream (`koharu-rs/koharu`, `koharu.rs`, Discord,
  `LICENSE-MIT`) et `CODE_OF_CONDUCT.md` ;
- le type de release release-please (`registerReleaseType('koharu')`) ;
- le dossier local `D:\codex\koharu`.

Version : `[workspace.package] version = "0.92.0-fork"` (Cargo.lock régénéré
par `cargo check`, aucune dépendance tierce mise à jour). Le manifeste
release-please reste à 0.92.0 : il ne sert pas à l'affichage et le tag est
posé à la main.

La traduction d'un dossier en lot reste en place, sans changement de
comportement : bouton « Open a folder… » de l'écran de démarrage
(`start.openFolder` -> commande Tauri `openFolder` -> `import::open_folder`),
et `koharu-batch run --input <dossier>` pour la ligne de commande.

`cargo check --workspace` et `cargo test --workspace --lib --bins --tests`
passent. `cargo test --workspace` échoue avant les tests sur un exemple
préexistant (`crates/koharu-ml/examples/qwen_image.rs:49`, `Option<f32>` reçu
là où un `f32` est attendu), sans lien avec le renommage.
## Distribution portable (complément release v0.92.0-fork)

`cargo tauri build --no-bundle` ne produit aucun installeur, et `koharu.exe`
seul ne démarre pas : c'est un binaire CEF, il lui faut `libcef.dll`, les
`.pak`, `icudtl.dat`, `bootstrap.exe`/`bootstrapc.exe` et `locales\` à côté
de lui (sinon « Could not find the webview runtime »). Le dossier racine du
dépôt contenait un runtime plus ancien que celui de `target\release`, donc
la release publie désormais une archive assemblée depuis `target\release` :

`KoharuBatch-0.92.0-fork-windows-x64-portable.zip` (602 Mo décompressés,
runtime CEF 156, 220 locales). Vérifiée en lançant l'exécutable extrait :
fenêtre « KoharuBatch », bouton « Open a folder… » présent.

Contenu : `koharu.exe`, `koharu-batch.exe`, `libcef.dll`, `chrome_elf.dll`,
`d3dcompiler_47.dll`, `dxcompiler.dll`, `icudtl.dat`, `resources.pak`,
`chrome_*.pak`, `v8_context_snapshot.bin`, `vk_swiftshader*`, `vulkan-1.dll`,
`bootstrap*.exe`, `CREDITS.html`, `locales\`.

Les deux `.exe` restent publiés à part : `koharu-batch.exe` est autonome,
`koharu.exe` ne sert que si le runtime est déjà présent. Aucune modification
de code pour ce complément : seul l'emballage change.
## Distribution portable 0.93.0 (build de validation)

`koharu.exe` seul n'est pas portable : c'est un binaire CEF, il exige
`libcef.dll`, les `.pak`, `icudtl.dat`, `bootstrap.exe`/`bootstrapc.exe` et
`locales\` à côté de lui. Le build 0.93.0 est donc livré en archive complète,
assemblée depuis `target\release` :

`dist\KoharuBatch-0.93.0-windows-x64-portable.zip` (277 613 356 o, 264,8 Mio ;
602,7 Mio décompressés, 242 fichiers), SHA-256 dans `dist\SHA256SUMS`.

Contenu : `koharu.exe` (0.93.0), `koharu-batch.exe` (requis par le dialogue de
lot, résolu à côté de l'application), runtime CEF 156 complet, 220 locales,
`LICENSE-MIT`, `LICENSE-APACHE`, `CREDITS.html`, `LISEZ-MOI.txt`.

Vérifié après extraction de l'archive dans un dossier vierge
(`target\portable-test`) : l'arbre extrait est identique au dossier assemblé
(242 fichiers, mêmes tailles), `koharu.exe` démarre depuis ce dossier avec la
fenêtre « KoharuBatch » et cinq processus CEF, `koharu-batch.exe --version`
répond. L'archive n'a pas été ouverte depuis l'Explorateur Windows ni testée
sur une autre machine.

### Config de release versionnée

L'override de build n'est plus dans `target\` (perdu au `cargo clean`) : il est
versionné dans `crates\koharu\tauri.release.conf.json` (cible NSIS,
`createUpdaterArtifacts: false`, sans version propre). Depuis la racine du
dépôt, avec ninja (VS BuildTools), cargo, rustc et bun dans le `PATH` :

    cargo tauri build --config crates/koharu/tauri.release.conf.json

La commande lance `bun run --filter @koharu/app build` puis produit
`target\release\bundle\nsis\koharu_0.93.0_x64-setup.exe`, sans `.sig`.
Vérifiée le 25/11 : `BUNDLE_EXIT=0`, installeur 0.93.0 (189,15 Mio ;
198 335 712 o), aucun artefact d'updater.

## Sidecar `koharu-batch` dans l'installeur (0.93.0)

L'installeur ne livrait pas `koharu-batch.exe` : le dialogue de lot échouait sur
une installation propre. Le CLI est désormais un `externalBin` Tauri
(`crates/koharu/tauri.conf.json`), préparé par `scripts/prepare-sidecar.ts`
(`bun run sidecar`, appelé par `beforeBuildCommand`) sous le nom
`crates/koharu/binaries/koharu-batch-<triple>[.exe]`, dossier ignoré par Git.

Vérifié : `installer.nsi` contient `File ... koharu-batch.exe` ; installation
propre dans `%LOCALAPPDATA%\koharu-clean-093` (NSIS currentUser, 0.93.0) avec
`koharu.exe` et `koharu-batch.exe` ; le CLI installé exécute les 8 étapes de
`test_pages` (PNG + `project.khr`) ; le dialogue, piloté sur le build debug avec
`KOHARU_BATCH_BIN` pointant sur le CLI installé, affiche `0/8 → 8/8 étapes` et
le toast « Lot terminé » sur deux passages. Le ZIP portable a été reconstruit
depuis `target\release` et contient les deux exe.
