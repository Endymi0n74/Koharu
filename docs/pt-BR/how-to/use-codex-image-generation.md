---
title: Usar Geração de Imagem com Codex
---

# Usar Geração de Imagem com Codex

O Koharu pode usar o Codex para geração image-to-image de ponta a ponta. Esse fluxo envia uma imagem de página de origem e um prompt ao Codex, depois salva a imagem gerada como resultado renderizado da página.

## Requisitos

- uma conta ChatGPT com acesso ao Codex
- autenticação de dois fatores habilitada nessa conta
- acesso de rede aos serviços da OpenAI e do ChatGPT

A autenticação de dois fatores precisa estar habilitada antes que o login por código de dispositivo possa ser concluído com sucesso.

## O que o recurso faz

A geração image-to-image do Codex é um fluxo de redesenho de página inteira. Ela pode usar a imagem de origem e o prompt para:

- traduzir o texto visível
- remover as letras originais
- redesenhar regiões editadas
- preservar layout dos painéis, balões, retículas e composição
- produzir uma imagem de página gerada em uma única passagem

Isso é separado do pipeline local em etapas do Koharu, no qual detecção, OCR, inpainting, tradução e renderização rodam como passos individuais. O fluxo do Codex envia a imagem da página para um serviço remoto e recebe uma imagem gerada como resultado.

## Prompt

Use um prompt que descreva o resultado final desejado para a página inteira. Por exemplo:

```text
Translate all visible text to natural English, remove the original lettering,
and redraw the page as a clean manga image while preserving the artwork,
panel layout, speech bubbles, tone, and composition.
```

Para edições mais estreitas, descreva a alteração desejada e o que precisa ser preservado. Como o modelo recebe a imagem da página de origem, o prompt deve focar nos objetivos de transformação em vez de repetir todos os detalhes visuais.

## Privacidade e confiabilidade

Esse recurso envia a imagem da página de origem e o prompt ao backend do ChatGPT Codex. Use o pipeline local quando precisar de processamento offline ou não quiser enviar imagens de páginas para um provedor remoto.

A geração de imagem do Codex depende do serviço upstream da OpenAI. Se a geração falhar, o Koharu mostra o texto de resposta upstream e o ID da requisição quando disponíveis. Tentar novamente pode resolver falhas transitórias. Falhas persistentes podem indicar limitações de acesso da conta, disponibilidade do serviço ou suporte do backend para chamadas da ferramenta de geração de imagem.

## Quando usar

Use a geração de imagem do Codex quando quiser um redesenho de ponta a ponta rápido e aceitar que um modelo remoto reescreva a imagem final.

Use o pipeline local em etapas quando quiser mais controle sobre OCR intermediário, máscaras de limpeza, texto traduzido, fontes e saída editável.
