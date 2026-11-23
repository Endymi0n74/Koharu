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
