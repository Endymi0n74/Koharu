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