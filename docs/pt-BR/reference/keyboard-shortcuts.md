---
title: Atalhos de teclado
---

# Atalhos de teclado

A maioria dos atalhos do editor pode ser personalizada em **Configurações > Atalhos**. Os padrões abaixo correspondem a uma instalação nova.

## Controles do canvas

Estes são baseados em gestos e não podem ser remapeados pelo usuário.

| Atalho | Ação |
| --- | --- |
| `Ctrl` + roda do mouse | Aumentar ou diminuir o zoom |
| `Ctrl` + arrastar | Deslocar a tela |
| Pinçar no trackpad | Zoom por pinça |

## Troca de ferramenta

| Padrão | Ação |
| --- | --- |
| `V` | Alterna para a ferramenta Selecionar |
| `M` | Alterna para a ferramenta Bloco (criação de blocos de texto) |
| `B` | Alterna para a ferramenta Pincel |
| `E` | Alterna para a ferramenta Borracha |
| `R` | Alterna para a ferramenta Pincel de Reparo |

## Tamanho do pincel

| Padrão | Ação |
| --- | --- |
| `]` | Aumenta o tamanho do pincel (limitado em 128) |
| `[` | Diminui o tamanho do pincel (limitado em 8) |

## Histórico e seleção

| Padrão | Ação |
| --- | --- |
| `Ctrl` + `Z` (`Cmd` + `Z` no macOS) | Desfazer |
| `Ctrl` + `Shift` + `Z` (`Cmd` + `Shift` + `Z` no macOS) | Refazer |
| `Ctrl` + `Y` | Refazer (fallback legado, não pode ser remapeado) |
| `Ctrl` + `A` (`Cmd` + `A` no macOS) | Seleciona todos os blocos de texto da página atual |

Desfazer e refazer disparam de propósito mesmo enquanto você digita num campo de texto — o histórico da cena tem precedência sobre o desfazer-de-texto nativo do navegador. `Ctrl + A` só dispara fora de campos de texto, então o comportamento nativo de "selecionar todo o texto" continua funcionando dentro de textareas e inputs.

## Personalizando atalhos

Abra **Configurações > Atalhos** para remapear qualquer um dos atalhos personalizáveis acima. Conflitos são destacados, e você pode redefinir tudo para os padrões na mesma tela.

Atalhos de troca de ferramenta e de tamanho de pincel só disparam quando o foco do teclado está fora de um campo de texto editável, então eles não interrompem a digitação nos painéis de OCR ou de tradução.
