# Handoff — Koharu (fork Endymi0n74/Koharu)

Reprise en session fraîche. Dernière mise à jour : 2026-11-21, après la re-vérification de chaque
hash et de chaque chiffre cité ici contre le dépôt, les balises et la PR #2 (tous conformes, sauf la
liste des commits non poussés, corrigée).

## Objectif

Rattraper `main` du fork sur `upstream/main` (koharu-rs/koharu) sans perdre le travail local,
rendre la CI verte, puis livrer `0.88.2` de bout en bout via release-please — **avec** ses binaires.

Contraintes : installations sous `D:\codex`, commits locaux sauf demande explicite de push, jamais
de PR (le merge est fait par l'utilisateur), aucun `Co-Authored-By`, ne pas réécrire l'historique ni
déplacer les balises existantes. Réponses en français, courtes, démarrant par « Stephane, ».

## Ce qui est fait et poussé

- Rebase des 152 commits du fork sur `upstream/main`, réparations Rust/docs, correctif Sentry
  (`14732df4`), tout poussé sur `origin`.
- CI : `paths-ignore: &code-irrelevant` (`*.md`, `packages/docs/**`, `.github/release-please/**`)
  sur build/test/lint (`c206fc9f`) ; `spec_decode_probe.rs` ignoré (`4c744faf`).
- `CHANGELOG.md` nettoyé, **poussé** : `5170f1e9` (fusion des deux `0.83.5`), `ffde708e` (fusion des
  deux `0.83.4` + suppression du lien bidon de `0.1.1`), `6525dba1` (bornes + ordre des sections),
  `08cdd9bb` (`0.83.5` remise juste sous `0.84.0`), puis `810c4f39` (bornes amont `0.81.10` /
  `0.25.3` restaurées : `0.82.0` pointe `0.81.10..0.82.0`, `0.25.4` pointe `0.25.3..0.25.4`, les
  anciennes bornes `0.81.9` / `0.25.2` avalaient leurs releases).
  La scission `0.14.4` (`3d1ed339`) ne fait **pas** partie de cette liste : elle reste locale.
- Audit des 4 premiers commits : `git diff -U0 1d90e6e8 08cdd9bb` = 6 hunks seulement ; 263 sections
  (265 avant), **1673 puces avant et après, 0 perdue, 0 ajoutée** ; **1673 commits distincts**
  (un par puce) tous présents dans le dépôt.
- `origin/main` = `810c4f39` ; les commits de ce fichier (à partir de `a9a584d3`) restent locaux —
  `git rev-list --count origin/main..HEAD` donne le nombre courant. La CI de `main` est verte hors
  jobs ignorés par `paths-ignore`.

## Ce qui est fait mais PAS poussé

- 5 commits locaux (`git rev-list --count origin/main..HEAD` = 5), à partir de `a9a584d3` :
  - `HANDOFF.md` seul : `a9a584d3`, `7b3cc71f`, `9422eb9e`, `ee3d9f86` ;
  - `CHANGELOG.md` seul : `3d1ed339` (scission `0.14.4` / `0.14.5`, option B, +10/−4).
- Tout le reste, dont `810c4f39`, est sur `origin/main` (= `810c4f39`).

## CHANGELOG — état vérifié

- **264 en-têtes**, **1673 puces**, **254 liens `compare`**, 10 en-têtes sans lien.
- 258 balises distinctes citées, **0 manquante** (`git rev-parse` sur chacune) ; chaque borne haute
  égale son titre au `v` près.
- Deux plages enjambent encore une balise réelle (`0.82.0` → `0.81.10`, `0.25.4` → `0.25.3`) et
  sont **identiques à l'amont** : ni ici ni en amont ces balises n'ont de section.
- `0.14.4` fait exception, volontairement (voir ci-dessous) : il a maintenant sa propre section.
- `0.81.9` et `0.25.2` gardent leurs propres sections (`0.81.8..0.81.9`, `0.25.1..0.25.2`) :
  aucune entrée orpheline.
- `0.83.5` est volontairement juste sous `0.84.0` : seule rupture d'ordre décroissant, assumée.
- Doublons traités : `0.83.5` 2 sections → 1 (2 puces), `0.83.4` 2 → 1 (3 puces). Pour `0.1.1`, les
  deux releases réelles sont conservées (mai : 121 puces ; avril : 25) ; seul le lien bidon
  `v0.1.1..0.1.1` a été retiré, l'en-tête restant sans lien comme chez l'amont.

### `0.14.4` — décision prise : option B (section scindée)

- Contexte : l'amont a créé `## [0.14.4]` dans `1b848334`, puis **a fusionné cette section dans
  `0.14.5`** dans `9aca5cda` (renommage du titre + 2 puces ajoutées). La balise `0.14.4`
  (`13e632b9`) existe pourtant bel et bien en amont, d'où le trou dans le chaînage.
- Choix retenu : recréer la section `0.14.4` (`compare/0.14.3..0.14.4`) avec ses **3 correctifs**
  (`b78f870`, `f9ac15c`, `8d4a00e`) et réduire `0.14.5` à `compare/0.14.4..0.14.5`
  (`e10c8ec`, `ee60acf`, `2be6f04`). La plage `0.14.3..0.14.5` couvrait 10 commits ; elle est
  maintenant coupée en deux sans qu'aucune puce ne bouge.
- **Divergence assumée avec l'amont** : à réappliquer si l'on resynchronise `CHANGELOG.md`.
- Vérifié après coup : 264 en-têtes, **1673 puces identiques à avant** (`set` de différence vide),
  254 liens `compare`, **0 borne haute ≠ titre**, **0 balise citée absente** (union local ∪ `upstream`
  ∪ `origin`), CRLF intégral, et l'ordre `0.15.0` → `0.14.5` → `0.14.4` → `0.14.3` respecté.

## Ce qui reste

1. **Merge de la PR #2 par l'utilisateur**, puis vérifier :
   - création de la balise `v0.88.2` par release-please ;
   - run `Release` (`.github/workflows/release.yml`) : c'est lui qui produit les binaires.
     `.github/sentry.sh` est corrigé sur `main` (upload conditionné au jeton + `SENTRY_ORG`), donc
     le build ne doit plus échouer sur l'upload.
2. **`v0.88.1` reste publiée sans artefacts** : `0.88.2` sera la première release à porter les
   binaires d'un arbre corrigé. Alternative si besoin : `workflow_dispatch` sur `release.yml`.

## PR #2 — « chore(main): release 0.88.2 »

- Tête `8086a4a7` (rebasée par release-please après `810c4f39`), `mergeable` = `MERGEABLE` ;
  `git merge-tree --write-tree origin/main refs/remotes/origin/pr2` → exit 0.
- Fichiers : `.github/release-please/manifest.json`, `CHANGELOG.md` (+17), `Cargo.lock`, `Cargo.toml`
  (`version = "0.88.2"`).
- L'entrée `0.88.2` (datée 2026-11-21, `compare/v0.88.1..v0.88.2`) liste **9 puces** : 6 docs
  (`810c4f3`, `08cdd9b`, `6525dba`, `ffde708`, `5170f1e`, `1d90e6e`) et 3 misc (`14732df`, `4c744fa`,
  `c206fc9`) — le commit du 2026-11-21 est désormais bien cité.
- Fusion simulée (tree `78ac23cc`) : `0.88.2` en tête, puis `0.88.1`, `0.88.0` ; **1682 puces**
  = 1673 + 9, **264 en-têtes**, **254 liens** `compare`.

## Fichiers clés

| chemin | rôle |
|---|---|
| `.github/release-please/{config.json,manifest.json,index.cjs}` | version via `cargo set-version`, tag et nom préfixés `v` |
| `.github/workflows/release-pr.yml` | ouvre la PR de release puis `release.yml --ref "$TAG"` |
| `.github/workflows/release.yml` | build + publication des binaires ; `SENTRY_ORG` transmis au build |
| `.github/sentry.sh` | upload conditionné à jeton + org, strip toujours exécuté |
| `.github/workflows/{build,test,lint}.yml` | `paths-ignore: &code-irrelevant` partagé |
| `Cargo.toml` | `[workspace.package] version`, seule source de version (bump par la PR #2) |
| `CHANGELOG.md` | 264 en-têtes, 254 liens `compare` ; seul écart voulu à l'amont : la section `0.14.4` |
| `packages/koharu/public/locales/` | 452 clés × 12 locales |
| `.gitignore` | `bench-profile/`, `spec_decode_probe.rs`, `/memory.md` |

## Questions ouvertes

1. **Binaires de `0.88.1`** : les laisser absents (`0.88.2` portera les binaires) ou rebâtir depuis
   `main` via un `workflow_dispatch` sur `release.yml` ?
2. **Captures d'écran** de l'aperçu d'erreur : dans `target/`, ignorées par Git. Dossier suivi voulu ?

## Notes d'environnement

- Dépôt `D:\Codex\koharu` ; `origin` = Endymi0n74/Koharu, `upstream` = koharu-rs/koharu.
- Balise `0.14.4` = `13e632b9`, **uniquement en amont** (`git ls-remote --tags upstream`), non
  récupérée localement ; `0.14.3` = `71dbaf47`, `0.14.5` = `9aca5cda`.
- Hashes `main` : `14732df4` → `c206fc9f` → `4c744faf` → `1d90e6e8` → `5170f1e9` → `ffde708e`
  → `6525dba1` → `08cdd9bb` → `810c4f39` (poussé le 2026-11-21).
- `gh` cible `koharu-rs/koharu` par défaut : **toujours** `-R Endymi0n74/Koharu` ; `gh api`
  n'accepte pas `-R` et écrit le JSON d'erreur sur stdout → juger sur `returncode`.
- `git fetch` d'une PR release-please doit être forcé (`--force`) : la branche est rebasée.
- `git merge-tree --write-tree <a> <b>` : exit 0 = pas de conflit.
- Python natif : `py -3`, `open(..., newline='')` pour préserver les CRLF,
  `sys.stdout.reconfigure(encoding='utf-8')`, scripts via heredoc.
- `CHANGELOG.md` est intégralement en CRLF : `git diff --check` doit rester propre après édition.
- PyYAML lit `on:` comme le booléen `True` : `data.get('on', data.get(True))`.
- Windows met ~40 min par jambe de build ; ne pas attendre les runs dans la session.
