# smart-commit

[![CI](https://github.com/Lucas-BRT/smart-commit/actions/workflows/ci.yml/badge.svg)](https://github.com/Lucas-BRT/smart-commit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/tag/Lucas-BRT/smart-commit?filter=v*&label=release&sort=semver)](https://github.com/Lucas-BRT/smart-commit/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.99-orange?logo=rust)](rust-toolchain.toml)
[![Shell](https://img.shields.io/badge/shell-bash-4EAA25?logo=gnubash&logoColor=white)](scripts/)
[![Platform](https://img.shields.io/badge/platform-linux%20x86__64-lightgrey?logo=linux&logoColor=white)](#instalação)
[![Devices](https://img.shields.io/badge/runs%20on-NPU%20%C2%B7%20GPU%20%C2%B7%20CPU-8A2BE2)](#devices)

Mensagens de commit **no estilo do seu repositório**, geradas por um LLM
pequeno rodando **na sua máquina**: NPU Intel, GPU (Intel, AMD ou NVIDIA) ou
CPU. Nada sai do computador, não há conta nem API, e o código é aberto, sob a
licença MIT.

```console
$ git add -p
$ smart-commit
feat(diff): adiciona a compactação por prioridade de arquivos
```

## Destaques

- **Segue o estilo do repositório.** Aprende com o histórico a convenção
  (Conventional Commits, gitmoji ou livre), o idioma (pt/en), os escopos e o
  ticket da branch.
- **Roda em qualquer acelerador.** OpenVINO para NPU, GPU Intel e CPU;
  llama.cpp com Vulkan para GPUs AMD e NVIDIA, com driver proprietário ou
  aberto. O `auto` mede a máquina e escolhe o mais rápido.
- **Funciona com qualquer cliente git.** `smart-commit hook install` preenche a
  mensagem de todo `git commit`, do terminal, do editor ou do lazygit, e
  nunca bloqueia um commit.
- **Reproduzível.** Toolchain, dependências, drivers, runtimes e modelos são
  fixados por versão e hash. A exportação do modelo e o tarball das releases
  dão os mesmos bytes em qualquer máquina, e `--trace`/`replay` refazem
  qualquer geração.

## Instalação

```bash
curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash
```

Instala em `~/.local`, sem `sudo`, e roda o setup guiado. O setup pergunta
antes de instalar drivers, baixa os modelos e termina com o benchmark, que
mostra qual device vai ser usado e quanto tempo cada um leva. Os detalhes e
as opções estão em [docs/uso.md](docs/uso.md).

Requisitos: Linux x86_64 (testado no Ubuntu/Pop!_OS 24.04). Os drivers são
instalados via `apt`; em outras distribuições o setup lista os pacotes
equivalentes.

<details>
<summary>A partir do código-fonte</summary>

```bash
git clone https://github.com/Lucas-BRT/smart-commit && cd smart-commit
cargo build --release --locked     # o toolchain vem do rust-toolchain.toml
scripts/setup.sh                   # drivers, modelos e benchmark
```
</details>

## Uso

| Comando | O que faz |
|---|---|
| `smart-commit` | gera a mensagem para o que está staged |
| `smart-commit --commit` | abre o editor com a mensagem e commita |
| `smart-commit hook install` | gera a mensagem em todo `git commit` do repositório |
| `smart-commit device` | mostra o device em uso e os tempos medidos |
| `smart-commit device use <npu\|gpu\|cpu\|vulkan\|auto>` | troca o device |
| `smart-commit bench` | mede os devices desta máquina |
| `smart-commit config list --show-origin` | mostra as configurações e de onde cada uma vem |
| `smart-commit context` / `prompt` | mostra o estilo detectado e o prompt exato |
| `smart-commit --trace t.json` / `replay t.json` | grava e refaz uma geração |

## Devices

| Device | Backend | Hardware | Ponta a ponta* |
|---|---|---|---|
| `gpu` | OpenVINO | GPU Intel (integrada ou Arc) | 1,8 s |
| `vulkan` | llama.cpp | GPU AMD (RADV), NVIDIA (proprietário ou NVK) ou Intel | 2,5 s |
| `cpu` | OpenVINO | qualquer x86_64 | 4,0 s |
| `npu` | OpenVINO | NPU Intel (Core Ultra) | 5,1 s, usando menos CPU |

\* Core Ultra 5 225H, Qwen2.5-Coder-1.5B, uma mensagem pela CLI. Na sua
máquina, `smart-commit bench` mede e escolhe.

## Como funciona

```
git diff --staged ─┐
git log ───────────┼─► estilo + diff compactado ─► prompt ─► modelo ─► saneamento ─► mensagem
branch ────────────┘   (cabe no limite da NPU)              (device)   (formato, 72 colunas,
                                                                          nova tentativa, fallback)
```

O design está em [docs/01-design.md](docs/01-design.md), e cada fase tem seu
documento:

- [01 — Design](docs/01-design.md)
- [02 — Fase 1: contexto do repositório](docs/02-fase-1-contexto.md)
- [03 — Fase 2: diff, prompt e saneamento](docs/03-fase-2-prompt.md)
- [04 — Fase 3: CLI e reprodutibilidade](docs/04-fase-3-cli.md)
- [05 — Fase 5: hook `prepare-commit-msg`](docs/05-fase-5-hook.md)
- [06 — Fase 4: backend na NPU](docs/06-fase-4-npu.md)
- [07 — Configuração](docs/07-configuracao.md)
- [08 — Plano: GPU, energia, qualidade e modelos](docs/08-plano-proximas-fases.md)
- [09 — Devices, benchmark, instalação e releases](docs/09-devices-instalacao.md)
- [Guia de uso](docs/uso.md)

## Desenvolvimento

```bash
cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked
cargo test --test hardware -- --ignored --test-threads 1    # modelo real (SMART_COMMIT_TEST_DEVICE=cpu|gpu|vulkan)
tests/scripts/test-setup-gpu.sh                              # scripts, sem hardware
```

As regras para contribuir (testes em todo commit, um objetivo por commit,
reprodutibilidade) estão em [AGENTS.md](AGENTS.md). Para lançar uma versão,
basta subir `version` no `Cargo.toml`: o CI testa, empacota e publica a
release.

## Licença

[MIT](LICENSE). Os modelos têm licença própria: o Qwen2.5-Coder é
Apache-2.0, e a licença vai junto com os arquivos baixados.
