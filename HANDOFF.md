# Handoff — Koharu (fork Endymi0n74/Koharu)

Reprise en session fraîche. État durable uniquement : les instantanés (têtes de PR, trees, compteurs,
chaînes de hashes) se lisent avec `git log`, `git rev-list --oneline origin/main..HEAD` et
`git merge-tree --write-tree origin/main refs/remotes/origin/pr2`.

## Objectif

Rattraper `main` du fork sur `upstream/main` (koharu-rs/koharu) sans perdre le travail local,
rendre la CI verte, puis livrer `0.88.2` de bout en bout via release-please — **avec** ses binaires.

Contraintes : installations sous `D:\codex`, commits locaux sauf demande explicite de push, jamais
de PR (le merge est fait par l'utilisateur), aucun `Co-Authored-By`, ne pas réécrire l'historique ni
déplacer les balises existantes. Réponses en français, courtes, démarrant par « Stephane, ».

## Fait et poussé sur `origin/main`

- Rebase des 152 commits du fork sur `upstream/main`, réparations Rust/docs, correctif Sentry dans
  `.github/sentry.sh` (upload conditionné au jeton + org, strip toujours exécuté).
- CI : `paths-ignore: &code-irrelevant` (`*.md`, `packages/docs/**`, `.github/release-please/**`)
  partagé par build/test/lint ; sonde `spec_decode_probe.rs` ignorée.
- `CHANGELOG.md` nettoyé (doublons `0.83.5` / `0.83.4` fusionnés, lien bidon de `0.1.1` retiré,
  bornes amont `0.81.10` → `0.82.0` et `0.25.3` → `0.25.4` restaurées, `0.83.5` remise sous
  `0.84.0`) et scission `0.14.4` / `0.14.5` appliquée.
- PR #2 revenue `MERGEABLE` après chaque rebase automatique de release-please ; la fusion simulée
  `git merge-tree` sort à 0. Les commits de ce fichier restent locaux tant qu'ils ne sont pas poussés.

## Décisions durables

- **Version** : `[workspace.package] version` de `Cargo.toml` est la seule source ; balises et noms de
  release préfixés `v`, gérés par release-please. Toute montée de version passe par la PR de release.
- **Release → binaires** : `.github/workflows/release.yml` n'a qu'un job, gardé par
  `if: github.ref_type == 'tag'`. Un `workflow_dispatch` sur une **branche** ne publie rien (job
  ignoré, run vert trompeur) ; sur une **balise**, il utilise `.github/workflows/release.yml` et
  `.github/sentry.sh` **de l'arbre de cette balise**, donc un correctif ultérieur ne s'y applique pas.
- **`v0.88.1` reste sans binaires**, assumé : son arbre figé porte le `sentry.sh` d'avant correctif
  et le dépôt n'a ni secret `SENTRY_AUTH_TOKEN` ni variable `SENTRY_ORG`, donc chaque jambe échoue en
  `beforeBundleCommand`. Secrets présents : `TAURI_SIGNING_PRIVATE_KEY`, sa phrase de passe.
  `0.88.2` est la première release dont l'arbre porte le correctif.
- **`0.14.4` — option B** : l'amont a créé `## [0.14.4]`, puis l'a fusionné dans `0.14.5` ; la balise
  `0.14.4` existe pourtant (en amont seulement, `13e632b9`). Le fork maintient `0.14.3..0.14.4`
  (3 correctifs : `b78f870`, `f9ac15c`, `8d4a00e`) et borne `0.14.5` à `0.14.4..0.14.5`
  (`e10c8ec`, `ee60acf`, `2be6f04`), sans ajouter ni perdre de puce. **Divergence assumée**, à
  réappliquer après tout resync via `py -3 scripts/fix-changelog-0.14.4.py` (`--check` pour vérifier ;
  le script refuse d'écrire si la zone contient une puce d'un autre commit).
- **Invariants `CHANGELOG.md`** à revérifier après édition : chaque borne haute égale son titre au `v`
  près, aucune balise citée n'est absente du dépôt, ordre décroissant sauf `0.83.5` juste sous
  `0.84.0` (volontaire), deux plages enjambent une balise sans section (`0.82.0`, `0.25.4`) comme
  en amont, et `0.1.1` garde ses deux releases réelles (mai : 121 puces, avril : 25) sans lien.

## Ce qui reste

1. **Merge de la PR #2 par l'utilisateur**, puis vérifier la création de la balise `v0.88.2` et le
   run `Release` : c'est lui qui produit les binaires (3 jambes : `ubuntu-latest`,
   `ubuntu-24.04-arm`, `windows-latest` ; macOS désactivé faute de certificat Apple). Compter ~40 min
   par jambe Windows, donc ne pas attendre dans la session.
2. Si le run échoue encore, lire le premier pas fautif avant de toucher au workflow ; le point Sentry
   est déjà neutralisé.

## Fichiers clés

| chemin | rôle |
|---|---|
| `.github/release-please/{config.json,manifest.json,index.cjs}` | version via `cargo set-version`, tag et nom préfixés `v` |
| `.github/workflows/release-pr.yml` | ouvre la PR de release puis lance `release.yml --ref "$TAG"` |
| `.github/workflows/release.yml` | build + publication des binaires ; garde `github.ref_type == 'tag'` |
| `.github/sentry.sh` | upload conditionné à jeton + org, strip toujours exécuté |
| `.github/workflows/{build,test,lint}.yml` | `paths-ignore: &code-irrelevant` partagé |
| `Cargo.toml` | `[workspace.package] version`, seule source de version |
| `CHANGELOG.md` | CRLF intégral ; seul écart voulu à l'amont : la section `0.14.4` |
| `scripts/fix-changelog-0.14.4.py` | rétablit la scission `0.14.4` / `0.14.5`, idempotent, `--check` |
| `packages/koharu/public/locales/` | traductions, une locale par dossier |
| `.gitignore` | `bench-profile/`, `spec_decode_probe.rs`, `/memory.md` |

## Questions ouvertes

1. **Captures d'écran** de l'aperçu d'erreur : elles vivent dans `target/`, ignorées par Git. Faut-il
   un dossier suivi ?

## Notes d'environnement

- Dépôt `D:\Codex\koharu` ; `origin` = Endymi0n74/Koharu, `upstream` = koharu-rs/koharu.
- `gh` cible `koharu-rs/koharu` par défaut : **toujours** `-R Endymi0n74/Koharu` ; `gh api` n'accepte
  pas `-R` et écrit le JSON d'erreur sur stdout → juger sur `returncode`.
- `git fetch` d'une PR release-please doit être forcé (`--force`) : la branche est rebasée.
- `git merge-tree --write-tree <a> <b>` : exit 0 = pas de conflit, première ligne = tree fusionné.
- `git show <rev>:<path>` exige `MSYS_NO_PATHCONV=1` sous Git Bash, sinon « ambiguous argument ».
- Python natif : `py -3`, `open(..., newline='')` pour préserver les CRLF,
  `sys.stdout.reconfigure(encoding='utf-8')`, scripts via heredoc.
- `CHANGELOG.md` est intégralement en CRLF : `git diff --check` doit rester propre après édition.
- PyYAML lit `on:` comme le booléen `True` : `data.get('on', data.get(True))`.
