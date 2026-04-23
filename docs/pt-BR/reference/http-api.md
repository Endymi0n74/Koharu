---
title: Referência da API HTTP
---

# Referência da API HTTP

O Koharu expõe uma API HTTP local em:

```text
http://127.0.0.1:<PORT>/api/v1
```

Esta é a mesma API usada pela UI desktop e pela Web UI em modo headless.

## Modelo de runtime

Comportamento atual importante:

- a API é servida pelo mesmo processo da GUI ou do runtime headless
- o servidor faz bind em `127.0.0.1` por padrão; use `--host` para fazer bind em outro lugar
- a API e o servidor MCP compartilham o mesmo projeto carregado, modelos e estado do pipeline
- quando nenhum `--port` é fornecido, o Koharu escolhe uma porta local aleatória
- tudo, exceto `/api/v1/downloads`, `/api/v1/operations` e `/api/v1/events`, retorna `503 Service Unavailable` até o app terminar a inicialização

## Modelo de recursos

A API é centrada em projetos. Apenas um projeto fica aberto por vez e contém:

- uma lista de `Pages` indexada por `PageId`
- `Nodes` por página (camadas de imagem, máscaras, blocos de texto) referenciados por `NodeId`
- um armazenamento `Blob` endereçado por conteúdo, que guarda os bytes brutos das imagens por hash Blake3
- um snapshot de `Scene` montado a partir dessas peças, avançado por um contador `epoch`
- um histórico de mutações `Op` que podem ser desfeitas ou refeitas

As mutações sempre passam pela camada de histórico (`POST /history/apply`), de modo que a cena, o autosave e os assinantes de eventos permaneçam sincronizados.

## Formatos comuns de response

Tipos de response frequentemente usados incluem:

- `MetaInfo` — versão do app e label do dispositivo de ML
- `EngineCatalog` — ids das engines instaláveis por etapa do pipeline
- `ProjectSummary` — id, nome, caminho, contagem de páginas, último acesso
- `SceneSnapshot` — `{ epoch, scene }`
- `LlmState` — estado atual de carga do LLM (status, target, error)
- `LlmCatalog` — modelos locais + de provedor agrupados por família
- `JobSummary` — `{ id, kind, status, error }`
- `DownloadProgress` — id do pacote, contagens de bytes, status

## Endpoints

### Meta

| Método | Path        | Finalidade                                          |
| ------ | ----------- | --------------------------------------------------- |
| `GET`  | `/meta`     | obtém a versão do app e o backend de ML ativo       |
| `GET`  | `/engines`  | lista as engines de pipeline registradas por etapa  |

### Fontes

| Método | Path                                  | Finalidade                                                       |
| ------ | ------------------------------------- | ---------------------------------------------------------------- |
| `GET`  | `/fonts`                              | catálogo combinado de fontes do sistema + Google Fonts para render |
| `GET`  | `/google-fonts`                       | catálogo do Google Fonts como uma lista isolada                  |
| `POST` | `/google-fonts/{family}/fetch`        | baixa e faz cache de uma família do Google Fonts                 |
| `GET`  | `/google-fonts/{family}/{file}`       | serve o arquivo TTF/WOFF em cache                                |

### Projetos

Todo projeto vive sob o diretório gerenciado `{data.path}/projects/`; clientes nunca fornecem caminhos do filesystem.

| Método   | Path                              | Finalidade                                                          |
| -------- | --------------------------------- | ------------------------------------------------------------------- |
| `GET`    | `/projects`                       | lista os projetos gerenciados                                       |
| `POST`   | `/projects`                       | cria um novo projeto (body `{ name }`)                              |
| `POST`   | `/projects/import`                | extrai um arquivo `.khr` em um diretório novo e o abre              |
| `PUT`    | `/projects/current`               | abre um projeto gerenciado por `id`                                 |
| `DELETE` | `/projects/current`               | fecha a sessão atual                                                |
| `POST`   | `/projects/current/export`        | exporta o projeto atual; retorna bytes binários                     |

`POST /projects/current/export` aceita `{ format, pages? }` onde `format` é um de `khr`, `psd`, `rendered`, `inpainted`. Quando o formato produz múltiplos arquivos, a response é `application/zip`.

### Páginas

| Método | Path                                    | Finalidade                                                  |
| ------ | --------------------------------------- | ----------------------------------------------------------- |
| `POST` | `/pages`                                | cria páginas a partir de N arquivos de imagem enviados (multipart) |
| `POST` | `/pages/from-paths`                     | caminho rápido só para Tauri que importa por caminho absoluto |
| `POST` | `/pages/{id}/image-layers`              | adiciona um node de imagem Custom a partir de um arquivo enviado |
| `PUT`  | `/pages/{id}/masks/{role}`              | faz upsert de um node de máscara a partir de bytes PNG brutos |
| `GET`  | `/pages/{id}/thumbnail`                 | obtém a thumbnail da página (cache em WebP)                 |

`role` é `segment` ou `brushInpaint`. `POST /pages` aceita um campo opcional `replace=true`; a importação é ordenada pelo nome de arquivo em ordem natural.

### Cena e blobs

| Método | Path                | Finalidade                                                              |
| ------ | ------------------- | ----------------------------------------------------------------------- |
| `GET`  | `/scene.json`       | snapshot completo da cena para clientes web/UI                          |
| `GET`  | `/scene.bin`        | `Snapshot { epoch, scene }` codificado em postcard para o cliente Tauri |
| `GET`  | `/blobs/{hash}`     | bytes brutos do blob por hash Blake3                                    |

`/scene.bin` inclui o epoch atual no header de response `x-koharu-epoch`.

### Histórico (mutações)

Todas as mutações da cena passam por aqui. Cada response retorna `{ epoch }`.

| Método | Path                | Finalidade                                  |
| ------ | ------------------- | ------------------------------------------- |
| `POST` | `/history/apply`    | aplica um `Op` (incluindo `Op::Batch`)      |
| `POST` | `/history/undo`     | reverte a última op aplicada                |
| `POST` | `/history/redo`     | reaplica a última op desfeita               |

`Op` é a união discriminada que cobre add/remove/update node, add/remove page, batch e outras transições da cena. O body é a variante com tag JSON.

### Pipelines

| Método | Path          | Finalidade                                       |
| ------ | ------------- | ------------------------------------------------ |
| `POST` | `/pipelines`  | inicia uma execução de pipeline como uma operação |

Campos do body:

- `steps` — ids das engines a executar em ordem (validados contra o registry)
- `pages` — subconjunto opcional de `PageId`s; omita para processar o projeto inteiro
- `region` — bounding box opcional para o inpainter (fluxo de pincel de reparo)
- `targetLanguage`, `systemPrompt`, `defaultFont` — overrides opcionais por execução

A response carrega um `operationId`. O progresso e a conclusão chegam em `/events` como `JobStarted`, `JobProgress`, `JobWarning` e `JobFinished`.

### Operações

`/operations` é o registry unificado para jobs em andamento e recém-concluídos (pipelines + downloads).

| Método   | Path                  | Finalidade                                                         |
| -------- | --------------------- | ------------------------------------------------------------------ |
| `GET`    | `/operations`         | snapshot de toda operação em andamento ou recente                  |
| `DELETE` | `/operations/{id}`    | cancela uma execução de pipeline; remoção best-effort para downloads |

### Downloads

| Método | Path                | Finalidade                                       |
| ------ | ------------------- | ------------------------------------------------ |
| `GET`  | `/downloads`        | snapshot de todo download ativo ou recente       |
| `POST` | `/downloads`        | inicia o download de um pacote de modelo (`{ modelId }`) |

`modelId` é um id de pacote declarado via `declare_hf_model_package!` (ex.: `"model:comic-text-detector:yolo-v5"`). A response é `{ operationId }`, reutilizando o id do pacote.

### Controle do LLM

O modelo carregado é um recurso singleton em `/llm/current`.

| Método   | Path             | Finalidade                                       |
| -------- | ---------------- | ------------------------------------------------ |
| `GET`    | `/llm/current`   | estado atual (status, target, error)             |
| `PUT`    | `/llm/current`   | carrega o target informado (local ou de provedor) |
| `DELETE` | `/llm/current`   | descarrega / libera o modelo                     |
| `GET`    | `/llm/catalog`   | lista os modelos locais + de provedor disponíveis |

`PUT /llm/current` aceita um `LlmLoadRequest`:

- targets de provedor — `{ kind: "provider", providerId, modelId }`
- targets locais — `{ kind: "local", modelId }`
- `options { temperature, maxTokens, customSystemPrompt }` opcional

`PUT /llm/current` retorna `204` assim que a tarefa de carga é enfileirada. O estado pronto efetivo é publicado como `LlmLoaded` em `/events`.

### Configuração

| Método   | Path                                    | Finalidade                                          |
| -------- | --------------------------------------- | --------------------------------------------------- |
| `GET`    | `/config`                               | lê o `AppConfig` atual                              |
| `PATCH`  | `/config`                               | aplica um `ConfigPatch`; persiste e faz broadcast   |
| `PUT`    | `/config/providers/{id}/secret`         | salva (ou sobrescreve) a chave de API de um provedor |
| `DELETE` | `/config/providers/{id}/secret`         | limpa a chave de API armazenada de um provedor      |

`AppConfig` expõe os top-level `data`, `http`, `pipeline` e `providers`:

- `data.path` — diretório de dados local usado para runtime, cache de modelos e projetos
- `http { connectTimeout, readTimeout, maxRetries }` — client HTTP compartilhado usado por downloads e requests baseados em provedor
- `pipeline { detector, fontDetector, segmenter, bubbleSegmenter, ocr, translator, inpainter, renderer }` — id da engine selecionada para cada etapa
- `providers[] { id, baseUrl?, apiKey? }` — chaves de API salvas vão e voltam como o placeholder redatado `"[REDACTED]"`; nunca o segredo bruto

Ids de provedores embutidos:

- `openai`
- `gemini`
- `claude`
- `deepseek`
- `deepl`
- `google-translate`
- `caiyun`
- `openai-compatible`

As chaves de API ficam armazenadas no credential store da plataforma, não em `config.toml`. Fazer PATCH de `apiKey: ""` limpa a chave salva; fazer PATCH de `"[REDACTED]"` mantém o valor inalterado. As rotas dedicadas `/config/providers/{id}/secret` são a forma explícita, fora do PATCH, de gerenciar o segredo de um provedor.

## Stream de eventos

O Koharu expõe um stream de Server-Sent Events em:

```text
GET /events
```

Comportamento:

- uma conexão nova (sem header `Last-Event-ID`) começa com um evento `Snapshot` contendo os registries atuais de jobs e downloads
- na reconexão, o servidor reenvia, em ordem, os eventos com `seq > Last-Event-ID` que ainda estão no buffer; se o id solicitado já saiu do ring, o servidor reenvia um `Snapshot`
- cada evento ao vivo é emitido com seu `seq` no campo `id:` do SSE
- um keepalive de 15 segundos é mantido

As variantes de evento atualmente incluem:

- `Snapshot` — estado completo inicial para clientes novos e em recuperação de atraso
- `JobStarted`, `JobProgress`, `JobWarning`, `JobFinished` — ciclo de vida do job de pipeline
- `DownloadProgress` — ticks de progresso de download de pacote
- `ConfigChanged` — config foi aplicada via `PATCH /config` ou via uma rota de segredo
- `LlmLoaded`, `LlmUnloaded` — transições do ciclo de vida do LLM
- `SceneAdvanced` — emitido quando uma mutação de cena avança o epoch

## Workflow típico

A ordem normal da API para um projeto novo é:

1. `POST /projects` — cria o projeto
2. `POST /pages` (ou `/pages/from-paths` no Tauri) — importa as imagens
3. `PUT /llm/current` — carrega um modelo de tradução (local ou de provedor)
4. `POST /pipelines` — dispara `detect → ocr → translate → inpaint → render`
5. acompanha `GET /events` até `JobFinished`
6. `POST /projects/current/export` com `format = "rendered"` ou `"psd"`

Para controle mais fino, faça `POST /history/apply` com payloads `Op` explícitos em vez de rodar um pipeline completo.

Se você prefere acesso orientado a agentes em vez de orquestrar endpoints HTTP, veja a [Referência das ferramentas MCP](mcp-tools.md).
