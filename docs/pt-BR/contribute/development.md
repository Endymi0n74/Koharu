---
title: Primeiros Passos
---

# Primeiros Passos

## Clonar o Repositório

```bash
git clone https://github.com/mayocream/koharu.git
cd koharu
```

## Pré-requisitos

- [Rust](https://www.rust-lang.org/tools/install) 1.92 ou mais recente
- [Bun](https://bun.sh/) 1.0 ou mais recente

### Windows

- Visual Studio C++ build tools
- [CUDA Toolkit 13.0](https://developer.nvidia.com/cuda-13-0-0-download-archive) para builds CUDA
- [AMD HIP SDK](https://www.amd.com/en/developer/resources/rocm-hub/hip-sdk.html) para ZLUDA

### macOS

- Xcode Command Line Tools (`xcode-select --install`)

### Linux

- Toolchain C/C++ da distro (`build-essential` ou equivalente)
- [LLVM](https://llvm.org/) 15 ou mais recente para builds com aceleração GPU

## Instalar Dependências

```bash
bun install
```

A toolchain Rust é resolvida automaticamente a partir do `rust-toolchain.toml` no primeiro build.

## Executar Localmente

```bash
bun run dev
```

Isso inicia o app Tauri em modo dev contra a UI empacotada.

## Build de Release

```bash
bun run build
```

Os binários ficam em `target/release-with-debug/` ou `target/release/` dependendo do profile.

## Comandos do Dia a Dia

Sempre use `bun cargo` para comandos Rust — é assim que as feature flags de plataforma (CUDA, Metal, Vulkan) são aplicadas corretamente.

```bash
bun cargo check                         # verificação de tipos
bun cargo clippy -- -D warnings         # lint
bun cargo fmt -- --check                # checagem de formatação
bun cargo test --workspace --tests      # testes unitários e de integração
```

Formatação de UI e configs usa [oxfmt](https://github.com/oxc-project/oxfmt):

```bash
bun run format
bun run format:check
```

Testes unitários da UI:

```bash
bun run test:ui
```

## Trabalho com ML

Ao iterar em `koharu-ml` ou `koharu-llm`, habilite o backend que bate com a sua máquina:

```bash
# Windows / Linux com NVIDIA
bun cargo test -p koharu-ml --features=cuda

# macOS (Apple Silicon)
bun cargo test -p koharu-ml --features=metal
```

Detalhes da seleção de backend estão em [Aceleração e Runtime](../explanation/acceleration-and-runtime.md).

## Documentação

A documentação mora em `docs/en-US/`, `docs/ja-JP/`, `docs/zh-CN/` e `docs/pt-BR/`. Construa cada locale que você editou:

```bash
zensical build -f docs/zensical.toml -c
zensical build -f docs/zensical.ja-JP.toml
zensical build -f docs/zensical.zh-CN.toml
zensical build -f docs/zensical.pt-BR.toml
```

Ao adicionar uma página nova, registre-a na nav do `zensical*.toml` correspondente.

## Antes de Abrir um PR

Rode só as checagens que batem com o que você mudou. Não precisa rodar tudo em todo PR.

- **Mudanças em Rust** — `bun cargo fmt -- --check`, `bun cargo check`, `bun cargo clippy -- -D warnings`, `bun cargo test --workspace --tests`
- **Mudanças na UI** — `bun run format`, `bun run test:ui`
- **Integração desktop** — `bun run build`
- **Docs** — compile cada locale que você editou

## Expectativas de PR

- **Um objetivo por PR.** Correção de bug *ou* refactor *ou* feature nova — não tudo junto.
- **Siga os padrões existentes.** Se o arquivo já tem uma convenção, combine com ela.
- **Descreva o que mudou e como verificou.** Screenshots ou clipes curtos para UI; before/after para pipeline.
- **Sem shims de retrocompatibilidade.** Substitua o código antigo no lugar — nada de pastas `v2/` nem aliases deprecated.
- **Sem refactors oportunistas.** Se achou algo a limpar sem relação, abra um PR separado.

PRs pequenos e focados são revisados mais rápido do que PRs grandes e misturados.

## Páginas Relacionadas

- [Build a Partir do Código-fonte](../how-to/build-from-source.md)
- [Executar nos Modos GUI, Headless e MCP](../how-to/run-gui-headless-and-mcp.md)
- [Solução de Problemas](../how-to/troubleshooting.md)
