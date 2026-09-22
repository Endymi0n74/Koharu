# Koharu — mémoire de projet

Fork de [koharu-rs/koharu](https://github.com/koharu-rs/koharu) (traducteur de manga ML, Rust).
Remotes : `origin` = https://github.com/Endymi0n74/Koharu (branche `main`), `upstream` = koharu-rs/koharu.

Machine de référence : **RTX 3070 8 Go (sm_86)**. Store : `%LOCALAPPDATA%\koharu\packages`.

Règles durables : [`AGENTS.md`](AGENTS.md). Ici : état du projet, décisions tranchées, pièges.

## Livré (sessions récentes)

- **Prompt enrichi** (`crates/koharu-translator/src/prompt.rs`) : `source_guidance` (JA/RU) +
  `target_style` (FR) — seul levier retenu pour la qualité JA/RU→FR (échantillonnage et pivot EN
  écartés). 81/81 tests translator.
- **`koharu-batch`** (`crates/koharu-pipeline/src/bin/koharu-batch.rs`) : exécution **phase-major**
  (un stage → un tour sur toutes les pages, les modèles restent chargés sur un chapitre),
  `--no-vision`, budget VRAM plafonné sur `free_bytes()` (`vram.rs`), `exit_with()`.
- **Fallback EN** : aide `--lang` + note dans `packages/docs/en/fork.mdx`.
- **Release** : `lto = "thin"` dans le profil release ; tag **`v0.83.5`** publié ; job **macOS
  désactivé** dans `release.yml` (secrets Apple absents) — réactiver quand
  `BUILD_CERTIFICATE_BASE64` / `KEYCHAIN_PASSWORD` seront configurés.

## Décisions tranchées (ne pas rouvrir sans re-mesurer sur la cible)

- **Flash attention** : défaut binaire `-1 (AUTO)` déjà actif sur CUDA. Le switch
  `KOHARU_FLASH_ATTN` a été **retiré** (mesure ON vs AUTO = bruit).
- **Réutilisation prefill/KV** : **non adoptée.** Mesuré : plafond ~12-15 % (préfill 1,52 s +
  1,40 s de retry sur un stage de 10,49 s, génération 4,25 s, ~3,3 s d'overhead), risque de sortie
  silencieusement fausse, budget 8 Go, concurrence des stages. Instrumentation retirée après mesure.
- **GUI pour `koharu-batch`** : **non** — CLI headless + app Tauri `koharu-app` existent déjà ;
  ne pas créer de 2ᵉ interface.
- **Exemple `crates/koharu-llama-sys/examples/flash_attn_default.rs`** : conservé (documente le
  défaut AUTO, aucun coût).

## Pièges

- **Le Lint CI est rouge en permanence, ce n'est PAS une régression.** `cargo fmt -- --check`
  (cmd exacte CI) sort 42 diffs / 14 fichiers, tous **préexistants** (identiques chez le parent du
  dernier commit). Correction : `cargo fmt --all` sur tout le dépôt — laissé au choix du
  propriétaire. Un nouveau commit peut être propre (0 nouveau fichier sale) tout en laissant le Lint
  rouge.
- **Toujours tester avec `--no-calibration`** ; ne jamais toucher
  `C:\Users\endymion\.koharu\vram-calibration.toml`.
- **« Exit code 1 » PowerShell après `git push`** = artefact stderr (git écrit la progression sur
  stderr), pas un échec : la ligne `... main -> main` confirme la réussite.
- **Smoke test** (exit attendu 0) :
  ```powershell
  .\target\release\koharu-batch.exe --input "crates\koharu-ml\benches\fixtures\object_detection" `
    --output "$env:TEMP\koharu-smoke\out" --llm gemma4-e4b-it `
    --report "$env:TEMP\koharu-smoke\report" --overwrite --no-calibration
  ```

## CI

| Workflow | Déclencheur | État |
|---|---|---|
| `build.yml` / `test.yml` | push `main` | verts |
| `koharu-batch.yml` | `main` + tag | vert |
| `lint.yml` | push `main` | **rouge (fmt préexistant)** |
| `release.yml` | tag `v*` | Windows / Ubuntu / ARM ✅ ; macOS désactivé |

**Release = tag `v*` déclenche `release.yml`.** Pousser sur `main` ne publie rien.

## À faire plus tard (choix ouverts)

- `cargo fmt --all` pour passer le Lint CI au vert.
- Réactiver le job macOS si les secrets Apple sont configurés.
- Mode dossier dans `koharu-app` (piloter un lot en GUI).
