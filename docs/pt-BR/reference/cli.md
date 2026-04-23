---
title: Referência da CLI
---

# Referência da CLI

Esta página documenta as opções de linha de comando expostas pelo binário desktop do Koharu.

O Koharu usa o mesmo binário para:

- inicialização do desktop
- Web UI local em modo headless
- a API HTTP local
- o servidor MCP embutido

## Uso comum

```bash
# macOS / Linux
koharu [OPTIONS]

# Windows
koharu.exe [OPTIONS]
```

## Opções

| Opção | Significado |
| --- | --- |
| `-d`, `--download` | Faz o prefetch das bibliotecas de runtime e da stack padrão de visão e OCR, e então encerra |
| `--cpu` | Força o modo CPU mesmo quando uma GPU está disponível |
| `-p`, `--port <PORT>` | Vincula o servidor HTTP local a uma porta `127.0.0.1` específica em vez de uma aleatória |
| `--host <HOST>` | Vincula o serviço HTTP a um host específico em vez de `127.0.0.1` |
| `--headless` | Executa sem iniciar a GUI desktop |
| `--debug` | Habilita saída de console orientada a debug |

## Notas de comportamento

Algumas flags afetam mais do que apenas a aparência inicial:

- sem `--port`, o Koharu escolhe uma porta local aleatória
- sem `--host`, o Koharu vincula apenas a `127.0.0.1`, então a API fica acessível somente a partir da mesma máquina
- com `--headless`, o Koharu pula a janela do Tauri mas ainda serve a Web UI e a API
- com `--download`, o Koharu encerra após o prefetch de dependências e não permanece em execução
- com `--cpu`, tanto a stack de visão quanto o caminho do LLM local evitam aceleração por GPU

Quando uma porta fixa está definida, os principais endpoints locais são:

- `http://localhost:<PORT>/`
- `http://localhost:<PORT>/api/v1`
- `http://localhost:<PORT>/mcp`

## Padrões comuns

Iniciar a Web UI em modo headless numa porta estável:

```bash
koharu --port 4000 --headless
```

Iniciar com inferência somente em CPU:

```bash
koharu --cpu
```

Baixar os pacotes de runtime antecipadamente:

```bash
koharu --download
```

Executar um endpoint MCP local numa porta estável:

```bash
koharu --port 9999
```

Depois conecte seu cliente MCP em:

```text
http://localhost:9999/mcp
```

Iniciar com logging explícito de debug:

```bash
koharu --debug
```

Vincular a todas as interfaces para que outras máquinas na rede local consigam alcançar a Web UI e a API:

```bash
koharu --host 0.0.0.0 --port 4000 --headless
```

Esse é o padrão prático para rodar o Koharu em um container ou VM, onde o cliente desktop vive em outro host. Qualquer coisa diferente de `127.0.0.1` fica acessível pela rede de forma deliberada, então só defina `--host` quando você realmente quiser acesso fora do loopback.
