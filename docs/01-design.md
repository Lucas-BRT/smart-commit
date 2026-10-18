# 01 — Design: mensagens de commit geradas localmente na NPU

> Status: proposta · Data: 2026-10-18
> Origem: pesquisa do projeto `ultra_linter` (`docs/01-pesquisa.md`), que já
> cobre driver da NPU, OpenVINO, restrições de LLM na NPU e bindings Rust.
> Este documento cobre só o que é específico deste projeto.

## 1. Proposta

Uma CLI em Rust que lê o diff staged e o histórico do repositório e gera uma
mensagem de commit **no estilo daquele repositório**, rodando um LLM pequeno
**localmente na NPU Intel**. Nada sai da máquina, e a geração não ocupa CPU nem
GPU.

Por que esse caso de uso combina com a NPU:

- é uma tarefa curta e eventual, então uma latência de alguns segundos é aceitável;
- é tolerante a imperfeição, porque o usuário revisa e edita a mensagem antes de confirmar;
- modelos de 0,5–4B conseguem resumir um diff razoavelmente bem.

Alternativas que já existem (aicommits, opencommit e similares) dependem de API
na nuvem ou de Ollama na CPU/GPU. O diferencial aqui é ser **local, de baixo
consumo e consciente do estilo do repositório**.

## 2. Fluxo

```
git diff --staged ──┐
git log -n 30 ──────┼─► contexto ─► prompt ─► Backend (NPU) ─► saneamento ─► mensagem
branch atual ───────┘   (estilo,                (GenAI)       (formato,
                         escopos,                              72 colunas,
                         ticket)                               sem ```)
```

### 2.1 "Com base no repositório": o que extrair

| Sinal | Fonte | Uso |
|---|---|---|
| Convenção (Conventional Commits, gitmoji, livre) | regex nos últimos ~30 assuntos | instrução de formato no prompt |
| Escopos usados (`feat(api):`) | mesmos commits | lista de escopos válidos |
| Idioma (pt/en) | heurística nos assuntos | instrução de idioma |
| Exemplos reais | 3–5 assuntos recentes | few-shot no prompt |
| Ticket (`ABC-123`) | nome da branch | prefixo/rodapé, se o repo usa |
| Arquivos alterados | `git diff --staged --stat` | sempre no prompt (barato) |

Tudo isso é **determinístico e testável sem a NPU**. É aqui que fica a maior
parte da lógica.

### 2.2 Limite de 1024 tokens da NPU

Diffs grandes não cabem no prompt. A estratégia vai em ordem crescente de custo:

1. **Compactar:** remover o contexto do diff (`-U1`), lockfiles, arquivos
   gerados e binários; manter o `--stat` completo.
2. **Truncar por prioridade:** distribuir o orçamento de tokens entre os
   arquivos, priorizando código-fonte a docs e testes.
3. **Map-reduce** (fase posterior): resumir cada arquivo separadamente e depois
   gerar a mensagem a partir dos resumos. Fica mais lento, então só vale para
   diffs grandes.
4. Opcional: subir o `MAX_PROMPT_LEN` para 2048–4096, medindo o custo.

### 2.3 Saneamento da saída

O modelo pequeno às vezes envolve a resposta em ```` ``` ````, coloca aspas,
explica o que fez ou passa de 72 colunas. Uma etapa determinística corrige isso
(e é coberta por testes de snapshot). Se a saída ainda for inválida, o programa
tenta uma vez mais; depois disso, usa como fallback um resumo baseado só no
`--stat`.

## 3. Integração

| Modo | Como | Observação |
|---|---|---|
| CLI | `npu-commit` → imprime a mensagem; `npu-commit --commit` → abre o editor com ela | o mais simples, primeiro a existir |
| Hook git | `npu-commit hook install` cria `prepare-commit-msg` | funciona com qualquer editor ou cliente git (Zed, terminal, lazygit). O hook **não faz nada** se a mensagem já veio com `-m`, no merge ou no amend |
| Daemon (opcional) | mantém o modelo carregado, e a CLI e o hook falam com ele por socket Unix | só se a carga do modelo for lenta demais **com** `CACHE_DIR`, decisão a tomar depois do benchmark |

## 4. Backend e portabilidade

- Trait `Backend { fn generate(&self, prompt: &Prompt) -> Result<String> }`.
- `GenaiBackend` (`openvino-genai`) com a escolha de device **NPU → GPU →
  CPU**: quem não tem NPU também consegue usar, só que mais devagar ou gastando
  mais bateria.
- `MockBackend` para os testes.
- Link com o OpenVINO **em runtime** (feature `runtime-linking`): o binário
  compila e roda sem o OpenVINO instalado, e nesse caso mostra uma mensagem
  clara de como instalar.
- Cache de compilação da NPU em `~/.cache/npu-commit/` e config em
  `~/.config/npu-commit/config.toml` (modelo, device, idioma, limites).
- Modelo: Qwen2.5-Coder-1.5B-Instruct ou Qwen3-1.7B em INT4 simétrico (formato
  exigido pela NPU). Comando `npu-commit model fetch` para baixar um modelo já
  exportado; verificar se existe versão compatível com NPU no Hugging Face ou
  publicar a nossa.

## 5. Testes incrementais

| Nível | Roda em | Cobre |
|---|---|---|
| Unitário | `cargo test` | detecção de convenção, escopos, idioma e ticket; compactação/truncamento do diff; montagem do prompt (snapshots `insta`); saneamento da saída |
| Integração | `cargo test` | repositórios git temporários (`tempfile`) com histórico sintético; `hook install` e uso real do hook com `MockBackend`; casos `-m`, merge e amend |
| Hardware | `cargo test -- --ignored` | NPU listada; geração real; tempos de carga com cache vazio e cheio, tempo até o 1º token e tokens/s |

Fixtures: alguns diffs reais (pequeno, médio, grande, só renomeação, só
lockfile) em `tests/fixtures/`.

## 6. Roadmap

| Fase | Entrega | Precisa da NPU? |
|---|---|---|
| 1 | Coleta de contexto do git + detecção de estilo | não |
| 2 | Compactação do diff + montagem do prompt + saneamento | não |
| 3 | CLI ponta a ponta com `MockBackend` | não |
| 4 | `GenaiBackend` na NPU + benchmark (`docs/06-fase-4-npu.md`) | **sim** (requer setup do driver/OpenVINO) |
| 5 | Hook `prepare-commit-msg` | não |
| 6 | Fallback de device, `model fetch`, config, empacotamento | parcial |
| 7 | (se o benchmark pedir) daemon / map-reduce | sim |

As fases 1–3 podem andar **em paralelo** com o setup da NPU, já que não dependem
dela.

## 7. Dependências (adicionar na fase em que forem usadas)

- Git: chamar o binário `git` via `std::process::Command`. É mais simples e
  respeita a config do usuário (diff drivers, `.gitattributes`). Alternativa
  futura: `gix`.
- `clap`, `serde` + `toml`, `regex`, `thiserror`/`anyhow`, `tracing`,
  `directories` (caminhos XDG).
- `openvino-genai` (fase 4).
- Dev: `insta`, `tempfile`, `assert_cmd`.
