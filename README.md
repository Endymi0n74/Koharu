<h1 align="center">Koharu</h1>

<p align="center">Traducteur de manga propulsé par le ML, écrit en <b>Rust</b>.</p>

<p align="center">
<a href="https://github.com/Endymi0n74/Koharu/releases/latest" target="_blank"><img alt="Dernière release" src="https://img.shields.io/github/v/release/Endymi0n74/Koharu?style=for-the-badge"></a>
<a href="https://github.com/Endymi0n74/Koharu/releases" target="_blank"><img alt="Téléchargements GitHub (toutes releases)" src="https://img.shields.io/github/downloads/Endymi0n74/Koharu/total?style=for-the-badge"></a>
<a href="https://github.com/Endymi0n74/Koharu/actions/workflows/koharu-batch.yml" target="_blank"><img alt="CI koharu-batch" src="https://github.com/Endymi0n74/Koharu/actions/workflows/koharu-batch.yml/badge.svg"></a>
</p>

<p align="center">
<a href="https://github.com/Endymi0n74/Koharu/releases" target="_blank">Releases</a> · <a href="https://koharu.rs/" target="_blank">Docs</a> · <a href="packages/docs/en/fork.mdx" target="_blank">Guide de la CLI par lot</a> · <a href="https://github.com/Endymi0n74/Koharu/issues" target="_blank">Rapports de bug</a> · <a href="https://discord.gg/mHvHkxGnUY" target="_blank">Discord</a>
</p>

<p align="center">
<a href="https://koharu.rs/ja" target="_blank">日本語</a> | <a href="https://koharu.rs/zh" target="_blank">简体中文</a>
</p>

**🇫🇷 Français** · [🇬🇧 English](README.en.md)

Koharu introduit un workflow local-first pour la traduction de manga, en exploitant la puissance du ML pour automatiser le processus. Il combine les capacités de détection d'objets, d'OCR, d'inpainting et des LLM afin de créer une expérience de traduction fluide.

> [!NOTE]
> Koharu exécute ses modèles de vision et ses LLM **localement** sur votre machine pour que vos données restent privées et sécurisées.

---

![capture d'écran de l'interface Koharu](packages/docs/screenshot.png)

## Fonctionnalités

- [Gestion de projets multi-formats](https://koharu.rs/en/guides/projects) pour images matricielles, archives et PDF, avec séquencement des pages
- [Pipeline sélectif](https://koharu.rs/en/guides/processing) pour la détection, l'OCR, la traduction et l'inpainting au niveau de la page ou du projet
- [Détection et segmentation](https://koharu.rs/en/guides/processing) des zones de texte, des bulles de dialogue et des zones de nettoyage
- [OCR multimodal](https://koharu.rs/en/models/vision) pour les dialogues, les légendes et le texte général de la page
- [Inférence GGUF locale et fournisseurs hébergés](https://koharu.rs/en/models/providers) pour les workflows LLM et de traduction automatique
- [Inpainting génératif](https://koharu.rs/en/guides/cleanup) pour la suppression du texte source et la reconstruction de l'illustration
- [Relecture](https://koharu.rs/en/guides/review) pour corriger les sorties de l'OCR et de la traduction
- [Canvas basé sur WebGPU](https://koharu.rs/en/guides/canvas) pour le nettoyage manuel, le placement du texte et la composition des pages
- [Mise en forme et disposition multilingues du texte](https://koharu.rs/en/guides/typesetting) avec ajustement automatique, repli de polices, CJK vertical et texte de droite à gauche
- [Export PSD en calques](https://koharu.rs/en/guides/export) pour une livraison aplatie et une édition en calques
- [Workflow basé sur des agents](https://koharu.rs/en/agent/projects) pour l'inspection et l'édition des projets, ainsi que le contrôle du pipeline

## Ce que cette branche ajoute

Ce dépôt est un fork de [koharu-rs/koharu](https://github.com/koharu-rs/koharu) : l'application d'origine reste intacte et gagne l'outillage autour d'elle.

- **La CLI par lot `koharu-batch`** — traduit des chapitres entiers depuis le terminal : un dossier de scans ou une archive CBZ en entrée, des pages traduites en sortie (`--lang` pour la cible), ou un **volume entier** où chaque sous-dossier ou `.cbz` devient son propre chapitre. Exécution **phase-major** (les modèles se chargent une seule fois au lieu d'une fois par page), reprise page par page y compris depuis une archive CBZ de sortie existante, `--no-vision` pour les exécutions en texte seul, rapport HTML/Markdown de fin d'exécution avec aperçus avant/après, résumé `--json` lisible par les machines, et code de sortie 0 seulement si toutes les pages ont réussi (prêt pour la CI).
- **Un choix automatique de modèle calibré** — le modèle le plus performant qui tient dans le budget de VRAM réellement disponible, avec calibration mesurée (`~/.koharu/vram-calibration.toml` enregistre les pics réels), **plancher de qualité 2 B** (refus actionnable sous le plancher, `--force` pour le contourner), **secours Qwen 2 B** quand les gemmas calibrés débordent du budget, et avertissement avant un premier gros téléchargement. `koharu-batch models` liste le catalogue avec les estimations de VRAM.
- **La reproductibilité** — `--deterministic` et `--torch-fp32` pour des sessions identiques d'un run à l'autre, `--ocr-cache` pour figer les lectures d'OCR, `--verify` pour contrôler les sorties, `--retries` pour relancer les segments qu'une réponse a laissés non traduits.
- **Le mode dossier dans l'application** — l'interface lance `koharu-batch` avec un sélecteur de modèle et de fournisseur hébergé, affiche le modèle en cours dans le centre d'activité, et remonte les erreurs en ligne.
- **Les fournisseurs hébergés dans le lot** — `--provider openai_compatible`, `lm_studio`, et les points d'accès compatibles OpenAI.
- **La maintenance du store** — `koharu-batch prune` repère les modèles orphelins, les datasets et les runtimes (ajoutez `--delete` pour les retirer) ; `KOHARU_STORE` déplace le store hors du disque système.
- **Un catalogue étendu** — les variantes non censurées Gemma 4 et Qwen (HauhauCS) en plus des modèles amont.

```bash
# Prévisualiser ce qui serait exécuté (pages, modèle, VRAM) sans rien lancer
koharu-batch --input ./chapter-12 --output ./chapter-12-fr --dry-run

# Un dossier de scans → un dossier de pages traduites
koharu-batch --input ./chapter-12 --output ./chapter-12-fr

# Une archive CBZ → une archive CBZ, avec le modèle choisi pour votre GPU
koharu-batch --input ./chapter-13.cbz --output ./chapter-13-fr.cbz

# Un volume entier : chaque sous-dossier et .cbz de ./series devient un chapitre
koharu-batch --input ./series --output ./series-fr

# Résumé lisible par les machines (fonctionne aussi avec --dry-run)
koharu-batch --input ./chapter-12 --output ./chapter-12-fr --json run.json

# Exécution texte seul : saute détection, OCR et inpainting
koharu-batch --input ./chapter-14 --output ./chapter-14-fr --no-vision

# Exécution reproductible : traduction greedy + étapes Torch en fp32 (détection, inpainting)
koharu-batch --input ./chapter-12 --output ./chapter-12-fr --deterministic --torch-fp32

# Lister les modèles locaux avec leurs estimations de VRAM (pics mesurés compris)
koharu-batch models
```

Voir [packages/docs/en/fork.mdx](packages/docs/en/fork.mdx) pour la documentation complète de la CLI.

## Accélération matérielle

Koharu prend en charge CUDA et ROCm / HIP sur Windows et Linux, Metal sur les puces Apple, et Vulkan sur Windows et Linux. Gardez votre pilote graphique à jour ; une installation complète du SDK CUDA ou ROCm n'est pas requise. Consultez [Exigences d'exécution et matérielles](https://koharu.rs/en/hardware) pour des recommandations propres à chaque modèle.

### CUDA

CUDA 13.3 nécessite un GPU NVIDIA de classe Turing ou plus récent, ainsi qu'un pilote R610 ou plus récent. Consultez la [matrice CUDA toolkit, pilotes et architectures](https://docs.nvidia.com/datacenter/tesla/drivers/cuda-toolkit-driver-and-architecture-matrix.html) officielle d'NVIDIA et installez le [dernier pilote NVIDIA](https://www.nvidia.com/en-us/drivers/).

### ROCm / HIP

La prise en charge de ROCm 10.0 dépend de la combinaison exacte du GPU AMD, du système d'exploitation et du pilote. Consultez la [matrice de compatibilité ROCm 10.0.0](https://rocm.docs.amd.com/en/docs-10.0.0/compatibility/compatibility-matrix.html) officielle d'AMD et installez un [pilote AMD](https://www.amd.com/en/support) compatible.

### Metal

Metal est disponible sur les Mac à puces Apple.

### Vulkan

Vulkan est disponible sur Windows et Linux en alternative à CUDA et ROCm / HIP.

### WebGPU

Le canvas de l'éditeur utilise WebGPU et nécessite un pilote graphique à jour, même lorsque l'inférence s'exécute sur le CPU.

### CPU

L'inférence sur CPU est disponible pour les charges de travail prises en charge, mais elle est nettement plus lente.

## Modèles d'apprentissage automatique

Koharu utilise des modèles distincts pour la détection, l'OCR, l'inpainting et la traduction. [Vision et inpainting](https://koharu.rs/en/models/vision) et [traduction et génération](https://koharu.rs/en/models/translation) ont des paramètres de modèle séparés.

### Modèles de vision par ordinateur

Les modèles de détection, d'OCR et d'inpainting sont sélectionnés séparément.

#### Détection et disposition

Le modèle de détection trouve les zones de texte, les bulles de dialogue et les masques de segmentation.

- [Koharu Layout RF-DETR Seg 2XL](https://huggingface.co/mayocream/koharu-layout-rfdetr-seg-2xl-1152)

#### OCR

L'OCR lit le texte source à partir des zones détectées.

- [PaddleOCR VL 1.6](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6)
- [Manga OCR](https://huggingface.co/mayocream/manga-ocr)
- [Baberu OCR](https://huggingface.co/genshiai-daichi/baberu-ocr)
- [Hayai OCR](https://huggingface.co/JustANormalTinkerer/hayai-ocr-v2)

#### Inpainting

L'inpainting reconstruit l'image derrière le texte source avant que la traduction ne soit rendue.

- [FLUX.2 Klein](https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF)
- [Qwen Image 2.1](https://huggingface.co/leejet/Qwen-Image-2.1-GGUF)
- [RORem mixed](https://huggingface.co/mayocream/RORem-mixed-GGUF)
- [LaMa](https://huggingface.co/mayocream/lama-manga)
- [AOT GAN](https://huggingface.co/mayocream/aot-inpainting)

### Grands modèles de langage

La traduction peut utiliser un modèle de langage local ou une API distante.

#### Modèles locaux généralistes

- Gemma 4 (QAT) : [gemma4-e2b-it](https://huggingface.co/unsloth/gemma-4-E2B-it-qat-GGUF), [gemma4-e4b-it](https://huggingface.co/unsloth/gemma-4-E4B-it-qat-GGUF), [gemma4-12b-it](https://huggingface.co/unsloth/gemma-4-12B-it-qat-GGUF), [gemma4-26b-a4b-it](https://huggingface.co/unsloth/gemma-4-26B-A4B-it-qat-GGUF), [gemma4-31b-it](https://huggingface.co/unsloth/gemma-4-31B-it-qat-GGUF)
- Qwen 3.5 : [qwen3.5-0.8b](https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF), [qwen3.5-2b](https://huggingface.co/unsloth/Qwen3.5-2B-GGUF), [qwen3.5-4b](https://huggingface.co/unsloth/Qwen3.5-4B-GGUF), [qwen3.5-9b](https://huggingface.co/unsloth/Qwen3.5-9B-GGUF), [qwen3.5-27b](https://huggingface.co/unsloth/Qwen3.5-27B-GGUF), [qwen3.5-35b-a3b](https://huggingface.co/unsloth/Qwen3.5-35B-A3B-GGUF)
- Qwen 3.6 : [qwen3.6-27b](https://huggingface.co/unsloth/Qwen3.6-27B-GGUF), [qwen3.6-35b-a3b](https://huggingface.co/unsloth/Qwen3.6-35B-A3B-GGUF)
- Qwen 3.8 : [qwen3.8-27b](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF)

#### Modèles locaux non censurés

- Gemma 4 non censuré : [gemma4-e2b-uncensored](https://huggingface.co/HauhauCS/Gemma-4-E2B-Uncensored-HauhauCS-Aggressive), [gemma4-e4b-uncensored](https://huggingface.co/HauhauCS/Gemma-4-E4B-Uncensored-HauhauCS-Aggressive), [gemma4-12b-uncensored](https://huggingface.co/HauhauCS/Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced), [gemma4-26b-a4b-uncensored](https://huggingface.co/HauhauCS/Gemma4-26B-A4B-QAT-Uncensored-HauhauCS-Balanced-MTP), [gemma4-31b-uncensored](https://huggingface.co/HauhauCS/Gemma4-31B-QAT-Uncensored-HauhauCS-Balanced-MTP)
- Qwen 3.5 non censuré : [qwen3.5-2b-uncensored](https://huggingface.co/HauhauCS/Qwen3.5-2B-Uncensored-HauhauCS-Aggressive), [qwen3.5-4b-uncensored](https://huggingface.co/HauhauCS/Qwen3.5-4B-Uncensored-HauhauCS-Aggressive), [qwen3.5-9b-uncensored](https://huggingface.co/HauhauCS/Qwen3.5-9B-Uncensored-HauhauCS-Aggressive)
- Qwen 3.6 non censuré : [qwen3.6-27b-uncensored](https://huggingface.co/HauhauCS/Qwen3.6-27B-Uncensored-HauhauCS-Balanced), [qwen3.6-35b-a3b-uncensored](https://huggingface.co/HauhauCS/Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive)
- Qwen 3.8 non censuré : [qwen3.8-27b-uncensored](https://huggingface.co/HauhauCS/Qwen3.8-27B-Uncensored-HauhauCS-Aggressive-MTP-GGUF)

#### Fournisseurs cloud

Fournisseurs LLM hébergés : [OpenAI](https://platform.openai.com/), [Gemini](https://ai.google.dev/), [Claude](https://www.anthropic.com/api), [Grok](https://docs.x.ai/developers), [MiniMax](https://platform.minimax.io/), [DeepSeek](https://platform.deepseek.com/) et [OpenRouter](https://openrouter.ai/).

#### Fournisseurs de traduction automatique

Fournisseurs de traduction automatique : [DeepL](https://www.deepl.com/), [Google Cloud Translation](https://cloud.google.com/translate) et [Caiyun](https://fanyi.caiyunapp.com/).

#### Fournisseurs compatibles OpenAI

Les points d'accès compatibles OpenAI sont également pris en charge.

## Installation

Téléchargez les builds depuis la [page des releases de ce dépôt](https://github.com/Endymi0n74/Koharu/releases/latest) :

- **Windows** — `koharu_*_x64-setup.exe` (installateur) ou `koharu_*_x64_en-US.msi`, plus `koharu-batch.exe` en binaire autonome ;
- **Linux** — AppImage, paquets `.deb` et `.rpm` (amd64 et arm64).

Chaque binaire est accompagné de sa signature `.sig` ; `latest.json` référence la version pour l'updater intégré. [Les prérequis d'installation et le premier lancement](https://koharu.rs/en/installation) varient selon le système d'exploitation.

> [!NOTE]
> Cette branche publie ses builds pour **Windows et Linux**. Sur macOS, construisez depuis les sources (voir **Développement** plus bas) : la jambe macOS de la CI est désactivée, mais Metal reste pris en charge.

## Dépannage

Les erreurs de démarrage, d'exécution, de modèle et de fournisseur sont traitées dans [Dépannage](https://koharu.rs/en/reference/troubleshooting). Définissez `RUST_LOG` sur `debug` ou `trace` pour des journaux verbeux :

```bash
# macOS / Linux
RUST_LOG=debug koharu
# Windows (PowerShell)
$env:RUST_LOG="debug"; koharu.exe
```

## Développement

Les dépendances de plateforme et les commandes de validation pour les builds locaux sont listées dans [Configuration du développement](https://koharu.rs/en/development/setup).

### Prérequis

- [Rust](https://www.rust-lang.org/tools/install) 1.97.1 ou version supérieure (édition Rust 2024)
- [Bun](https://bun.sh/) 1.3.14 ou version supérieure
- [LLVM](https://llvm.org/) 22.1.8 ou version supérieure

### Installer les dépendances

```bash
bun install
```

### Lancer en développement

```bash
bun dev
```

### Compiler

```bash
bun run build
```

L'exécutable est écrit dans `target/release`.

### Validation

Commandes de validation du dépôt :

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace --tests
bun run lint
bun run test
bun run --filter '@koharu/*' typecheck
bun scripts/check-path-portability.ts
```

## Sponsoring

Si Koharu est utile dans votre flux de travail, envisagez de soutenir le projet.

- [GitHub Sponsors](https://github.com/sponsors/mayocream)
- [Patreon](https://www.patreon.com/mayocream)

![sponsors](./.github/sponsorkit/sponsors.svg)

## Contributeurs ❤️

Merci à toutes les personnes qui ont aidé à rendre Koharu meilleur !

<a href="https://github.com/Endymi0n74/Koharu/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=Endymi0n74/Koharu" />
</a>

## Licence

Copyright 2025-2026 Mayo Takanashi et les contributeurs de Koharu.

Koharu est sous licence double : la [Licence MIT](LICENSE-MIT) ou la
[Apache License, version 2.0](LICENSE-APACHE), à votre convenance.
