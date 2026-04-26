---
title: Modelos e Provedores
---

# Modelos e Provedores

O Koharu usa tanto modelos de visão quanto modelos de linguagem. O stack de visão prepara a página; o stack de linguagem lida com a tradução.

Se você quer a visão ao nível arquitetural de como essas peças se encaixam, leia [Mergulho Técnico Profundo](technical-deep-dive.md) depois desta página.

## Modelos de visão

O Koharu baixa automaticamente os modelos de visão necessários na primeira vez que você os usa.

O stack padrão atual inclui:

- [comic-text-bubble-detector](https://huggingface.co/ogkalu/comic-text-and-bubble-detector) para detecção conjunta de blocos de texto e balões de fala
- [comic-text-detector](https://huggingface.co/mayocream/comic-text-detector) para máscaras de segmentação de texto
- [PaddleOCR-VL-1.5](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.5) para reconhecimento de texto por OCR
- [aot-inpainting](https://huggingface.co/mayocream/aot-inpainting) para o inpainting padrão
- [YuzuMarker.FontDetection](https://huggingface.co/fffonion/yuzumarker-font-detection) para detecção de fonte e cor

Alguns modelos são usados diretamente dos repositórios upstream do Hugging Face, enquanto os pesos convertidos em `safetensors` são hospedados no [Hugging Face](https://huggingface.co/mayocream) quando o Koharu precisa de um pacote amigável para Rust.

### O que é cada modelo de visão

| Modelo                        | Tipo de modelo          | Por que o Koharu o usa                                     |
| ---------------------------- | ---------------------- | ------------------------------------------------------- |
| `comic-text-bubble-detector` | detector de objetos    | encontra blocos de texto e regiões de balão de fala em uma única passagem |
| `comic-text-detector`        | rede de segmentação    | produz uma máscara de texto para limpeza                    |
| `PaddleOCR-VL-1.5`           | modelo de linguagem visual  | lê texto recortado em tokens de texto                     |
| `aot-inpainting`             | rede de inpainting     | reconstrói regiões de imagem mascaradas após a remoção do texto    |
| `YuzuMarker.FontDetection`   | classificador / regressor | estima dicas de fonte e estilo para a renderização            |

A escolha de design importante é que o Koharu não usa um modelo para cada tarefa de página. Detecção, segmentação, OCR e inpainting precisam de formatos de saída diferentes:

- a detecção conjunta quer blocos de texto e regiões de balão
- a segmentação quer máscaras por pixel
- o OCR quer texto
- o inpainting quer pixels restaurados

### Alternativas internas opcionais

Você pode trocar estágios individuais em **Configurações > Engines**. As alternativas internas incluem:

- [PP-DocLayoutV3](https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors) como detector alternativo e engine de análise de layout
- [speech-bubble-segmentation](https://huggingface.co/mayocream/speech-bubble-segmentation) como detector dedicado de balões
- [Manga OCR](https://huggingface.co/mayocream/manga-ocr) e [MIT 48px OCR](https://huggingface.co/mayocream/mit48px-ocr) como engines de OCR alternativos
- [FLUX.2 Klein 4B](https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF) como inpainter opcional baseado em FLUX.2
- [lama-manga](https://huggingface.co/mayocream/lama-manga) como inpainter alternativo

## LLMs locais

O Koharu suporta modelos GGUF locais através do [llama.cpp](https://github.com/ggml-org/llama.cpp). Esses modelos rodam na sua máquina e são baixados sob demanda quando você os seleciona no seletor de LLM.

Na prática, os modelos locais geralmente são transformers decoder-only quantizados. GGUF é o formato do modelo; `llama.cpp` é o runtime de inferência.

### Modelos locais internos focados em tradução para saída em inglês

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf): um GGUF Q5_K_M, melhor quando a qualidade da tradução importa mais
- [lfm2.5-1.2b-instruct](https://huggingface.co/LiquidAI/LFM2.5-1.2B-Instruct-GGUF): uma opção menor multilíngue do tipo instruct para sistemas com pouca memória ou iteração mais rápida
- [sugoi-14b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-14B-Ultra-GGUF) e [sugoi-32b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-32B-Ultra-GGUF): escolhas maiores orientadas para tradução quando você quer mais folga

### Modelos locais internos focados em tradução para saída em chinês

- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7): uma escolha balanceada entre qualidade e velocidade em GPUs da classe de 8 GB
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX): uma opção mais leve para configurações de médio porte ou com uso intenso de CPU

### Modelo local interno focado em tradução para cobertura de idiomas mais ampla

- [hunyuan-mt-7b](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF): uma opção multi-idioma com requisitos moderados de hardware

### Outras famílias de modelos locais internos

O seletor local também inclui famílias de propósito geral que não são específicas para tradução:

- Gemma 4 instruct: `gemma4-e2b-it`, `gemma4-e4b-it`, `gemma4-26b-a4b-it`, `gemma4-31b-it`
- Gemma 4 uncensored: `gemma4-e2b-uncensored`, `gemma4-e4b-uncensored`
- Qwen 3.5: `qwen3.5-0.8b`, `qwen3.5-2b`, `qwen3.5-4b`, `qwen3.5-9b`, `qwen3.5-27b`, `qwen3.5-35b-a3b`
- Qwen 3.5 uncensored: `qwen3.5-2b-uncensored`, `qwen3.5-4b-uncensored`, `qwen3.5-9b-uncensored`, `qwen3.5-27b-uncensored`, `qwen3.5-35b-a3b-uncensored`
- Qwen 3.6: `qwen3.6-27b`, `qwen3.6-35b-a3b`
- Qwen 3.6 uncensored: `qwen3.6-27b-uncensored`, `qwen3.6-35b-a3b-uncensored`

## Provedores remotos

O Koharu também pode traduzir através de APIs remotas ou auto-hospedadas em vez de baixar um modelo local.

As famílias de provedores suportados são:

- baseados em LLM: `OpenAI`, `Gemini`, `Claude`, `DeepSeek`, mais qualquer endpoint `OpenAI-compatible` que exponha `/v1/models` e `/v1/chat/completions` (LM Studio, OpenRouter, vLLM, etc.)
- tradução automática: `DeepL`, `Google Cloud Translation`, `Caiyun`

Provedores de tradução automática são serviços de tradução puros, não modelos de chat. Eles recebem o texto fonte e um idioma de destino e devolvem uma tradução; não há system prompt nem seletor de modelo.

### Modelos remotos de LLM internos atuais

Os padrões internos atuais para os provedores baseados em LLM são:

- OpenAI: `gpt-5-mini` (`GPT-5 mini`)
- Gemini: `gemini-3.1-flash-lite-preview` (`Gemini 3.1 Flash-Lite Preview`)
- Claude: `claude-haiku-4-5` (`Claude Haiku 4.5`)
- DeepSeek: `deepseek-chat` (`DeepSeek-V3.2-Chat`)
- APIs compatíveis com OpenAI: os modelos são descobertos dinamicamente a partir do endpoint configurado

### Provedores de tradução automática

| Provedor | O que você precisa | Notas |
| --- | --- | --- |
| `DeepL` | Chave de API do DeepL | Base URL customizada opcional para os endpoints do DeepL Pro vs. Free |
| `Google Cloud Translation` | Chave de API do Google Cloud | Usa o endpoint REST v2 |
| `Caiyun` | Token do Caiyun | Cobertura limitada de idiomas de destino |

Os provedores remotos são configurados em **Configurações > Chaves de API**.

Para um guia passo a passo de configuração para LM Studio, OpenRouter e endpoints similares, veja [Usar APIs Compatíveis com OpenAI](../how-to/use-openai-compatible-api.md).

### Geração de imagem com Codex

O Koharu também pode usar o Codex para geração image-to-image de ponta a ponta. Em vez de traduzir blocos de texto e renderizar texto localmente como etapas separadas, esse fluxo envia a imagem de página de origem e o prompt ao Codex e recebe uma imagem de página gerada.

Esse é um fluxo remoto de geração de imagem, não um modelo local. Ele exige uma conta ChatGPT com acesso ao Codex e autenticação de dois fatores habilitada para concluir o login por código de dispositivo. Consulte [Usar Geração de Imagem com Codex](../how-to/use-codex-image-generation.md) para notas de uso e limitações.

## Escolhendo entre local e remoto

Use modelos locais quando você quer:

- a configuração mais privada
- operação offline após a conclusão dos downloads
- maior controle sobre o uso de hardware

Use provedores remotos quando você quer:

- evitar downloads grandes de modelos locais
- reduzir o uso local de VRAM ou RAM
- conectar-se a um serviço de modelos hospedado ou auto-gerenciado

!!! note

    Quando você usa um provedor remoto, o Koharu envia o texto do OCR selecionado para tradução ao provedor que você configurou.

## Leitura de fundo

Para a teoria de fundo por trás das categorias de modelos desta página, veja:

- [Mergulho Técnico Profundo](technical-deep-dive.md)
- [Transformada de Fourier na Wikipédia](https://en.wikipedia.org/wiki/Fourier_transform)
- [Image segmentation na Wikipédia](https://en.wikipedia.org/wiki/Image_segmentation)
- [OCR na Wikipédia](https://en.wikipedia.org/wiki/Optical_character_recognition)
- [Arquitetura Transformer na Wikipédia](<https://en.wikipedia.org/wiki/Transformer_(deep_learning_architecture)>)
