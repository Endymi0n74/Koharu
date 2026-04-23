---
title: Configurar Clientes MCP
---

# Configurar Clientes MCP

O Koharu expõe um servidor MCP embutido via Streamable HTTP local. Esta página mostra como conectar clientes MCP a ele, com setup concreto para Antigravity, Claude Desktop e Claude Code.

## O que o Koharu expõe via MCP

O servidor MCP do Koharu é o mesmo runtime local usado pelo app desktop e pela Web UI headless. A superfície de tools atual é deliberadamente pequena e centrada no ciclo de vida do projeto, na camada de histórico e nos jobs de pipeline:

- `koharu.open_project` / `koharu.close_project`
- `koharu.apply` / `koharu.undo` / `koharu.redo`
- `koharu.start_pipeline`

Para inspeção e edição mais ricas — snapshots de cena, thumbnails de página, fetch de blobs, listas de fontes, controle do LLM, exports, configuração — os agentes chamam diretamente a API HTTP do Koharu em `http://127.0.0.1:<PORT>/api/v1`. A API HTTP e o servidor MCP compartilham o mesmo processo e o mesmo estado, então um agente pode misturar os dois livremente em um único workflow.

Para a lista completa de tools e os schemas de parâmetros, veja [Referência das ferramentas MCP](../reference/mcp-tools.md).

## 1. Inicie o Koharu em uma porta estável

Use uma porta fixa para que seu cliente MCP sempre tenha a mesma URL.

```bash
# macOS / Linux
koharu --port 9999 --headless

# Windows
koharu.exe --port 9999 --headless
```

Você também pode manter a janela desktop aberta e ainda expor o MCP:

```bash
# macOS / Linux
koharu --port 9999

# Windows
koharu.exe --port 9999
```

O endpoint MCP do Koharu será então:

```text
http://127.0.0.1:9999/mcp
```

Detalhes importantes:

- mantenha o Koharu rodando enquanto o cliente MCP estiver conectado
- o Koharu faz bind em `127.0.0.1` por padrão, então esses exemplos assumem que o cliente MCP está na mesma máquina
- não são necessários headers de autenticação para o setup local padrão

## 2. Verificação rápida de endpoint

Antes de editar qualquer configuração de cliente, confirme que o Koharu está realmente rodando na porta esperada.

Abra:

```text
http://127.0.0.1:9999/
```

Se a Web UI carregar, o servidor local está de pé e o endpoint MCP também deve existir em `/mcp`.

## Antigravity

O Antigravity pode apontar diretamente para a URL MCP local do Koharu através da configuração raw de MCP.

### Passos

1. Inicie o Koharu com `--port 9999`.
2. Abra o Antigravity.
3. Abra o menu `...` no topo do painel de agente do editor.
4. Clique em **Manage MCP Servers**.
5. Clique em **View raw config**.
6. Adicione uma entrada `koharu` em `mcpServers`.
7. Salve a configuração.
8. Reinicie o Antigravity se ele não recarregar o servidor MCP automaticamente.

### Exemplo de config

```json
{
  "mcpServers": {
    "koharu": {
      "serverUrl": "http://127.0.0.1:9999/mcp"
    }
  }
}
```

Se você já tem outros servidores MCP configurados, adicione `koharu` junto deles em vez de substituir o objeto `mcpServers` inteiro.

### Depois do setup

Peça ao Antigravity algo simples primeiro:

- `What Koharu MCP tools do you have available?`
- `Open the Koharu project at C:\\projects\\my-manga.khrproj.`

Se isso funcionar, avance para trabalho real, como:

- `Open the project at C:\\projects\\my-manga.khrproj and start a pipeline with steps detect, ocr, llm-translate, aot-inpainting, koharu-renderer.`
- `Undo the last edit in Koharu.`
- `Apply this Op to add a new text block to page <id>: { ... }`

## Claude Desktop

A configuração atual de MCP local do Claude Desktop é baseada em comando. Como o Koharu expõe um endpoint HTTP MCP local em vez de uma desktop extension empacotada, a abordagem prática é usar um pequeno processo bridge que conecta o Claude Desktop a `http://127.0.0.1:9999/mcp`.

Este guia usa o `mcp-remote` como bridge.

### Antes de começar

Certifique-se de que uma destas condições seja verdadeira:

- `npx` já está disponível na sua máquina
- o Node.js está instalado para que o `npx` possa rodar

### Passos

1. Inicie o Koharu com `--port 9999`.
2. Abra o Claude Desktop.
3. Abra **Settings**.
4. Abra a seção **Developer**.
5. Abra o arquivo de configuração MCP a partir da entrada do editor embutido do Claude Desktop.
6. Adicione uma entrada de servidor `koharu`.
7. Salve o arquivo.
8. Reinicie completamente o Claude Desktop.

### Config para Windows

```json
{
  "mcpServers": {
    "koharu": {
      "command": "C:\\Progra~1\\nodejs\\npx.cmd",
      "args": [
        "-y",
        "mcp-remote@latest",
        "http://127.0.0.1:9999/mcp"
      ],
      "env": {}
    }
  }
}
```

### Config para macOS / Linux

```json
{
  "mcpServers": {
    "koharu": {
      "command": "npx",
      "args": [
        "-y",
        "mcp-remote@latest",
        "http://127.0.0.1:9999/mcp"
      ],
      "env": {}
    }
  }
}
```

Notas:

- se você já tem outras entradas em `mcpServers`, adicione `koharu` sem apagá-las
- o `mcp-remote@latest` é baixado no primeiro uso, então a primeira inicialização pode precisar de acesso à internet
- se sua instalação do Node no Windows não estiver em `C:\\Program Files\\nodejs`, atualize o caminho em `command` adequadamente
- o fluxo atual do conector remote-MCP da Anthropic para o Claude Desktop é gerenciado por **Settings > Connectors** para servidores remotos reais; esta página intencionalmente cobre o padrão bridge via arquivo de configuração para o endpoint local `127.0.0.1` do Koharu

### Depois do setup

Abra um novo chat no Claude Desktop e pergunte:

- `What Koharu MCP tools do you have available?`
- `Open the Koharu project at D:\\projects\\my-manga.khrproj.`

Depois passe para trabalho real:

- `Run a Koharu pipeline with steps detect, ocr, llm-translate, aot-inpainting, koharu-renderer on the project I just opened.`
- `Use Koharu's HTTP API at http://127.0.0.1:9999/api/v1/operations to check pipeline status.`
- `Use Koharu's HTTP API to export the project as PSD.`

## Claude Code

Se por "Claude" você quer dizer Claude Code, o setup mais seguro para o endpoint MCP local `http://127.0.0.1` do Koharu é usar o mesmo padrão de bridge stdio.

### Adicionar à sua config de usuário

macOS / Linux:

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"npx\",\"args\":[\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

Isso grava o servidor na configuração MCP do Claude Code para a sua conta de usuário.

Windows:

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"cmd\",\"args\":[\"/c\",\"npx\",\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

No Windows nativo, a documentação do Claude Code recomenda explicitamente o wrapper `cmd /c npx` para servidores MCP locais em stdio que usam `npx`.

### Verificar

```bash
claude mcp get koharu
claude mcp list
```

Se você já configurou o Koharu no Claude Desktop, o Claude Code também consegue importar entradas compatíveis do Claude Desktop em plataformas suportadas:

```bash
claude mcp add-from-claude-desktop --scope user
```

## Primeiras tarefas para testar

Depois que o cliente estiver conectado, estas são boas primeiras tarefas:

- perguntar ao agente quais tools MCP do Koharu estão disponíveis
- abrir um diretório de projeto Koharu existente
- iniciar um pipeline com uma lista pequena de steps (ex.: `detect`, `ocr`)
- pedir ao agente para ler `GET /api/v1/scene.json` via HTTP para inspecionar o resultado antes de rodar o pipeline completo

Misturar a pequena superfície de tools MCP com chamadas HTTP diretas é intencional — mantém a superfície do protocolo enxuta e ainda dá aos agentes acesso ao estado completo do editor.

## Erros comuns

- iniciar o Koharu sem `--port` e depois tentar conectar um cliente na porta errada
- usar `http://127.0.0.1:9999/` em vez de `http://127.0.0.1:9999/mcp`
- fechar o Koharu depois de adicionar a configuração do cliente
- substituir toda a configuração do seu cliente em vez de mesclar uma nova entrada `koharu`
- esperar que o Claude Desktop conecte diretamente à URL HTTP do Koharu através de uma entrada de configuração simples sem comando
- esquecer que o servidor local padrão do Koharu só é alcançável na mesma máquina

## Páginas relacionadas

- [Executar nos Modos GUI, Headless e MCP](run-gui-headless-and-mcp.md)
- [Referência de MCP Tools](../reference/mcp-tools.md)
- [Referência da CLI](../reference/cli.md)
- [Troubleshooting](troubleshooting.md)

## Referências externas

- [Documentação de MCP do Claude Code](https://code.claude.com/docs/en/mcp)
- [Ajuda do Claude: Building custom connectors via remote MCP servers](https://support.claude.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers)
- [Artigo de suporte da Wolfram com exemplos atuais de config MCP para Antigravity e Claude Desktop](https://support.wolfram.com/73463/)
