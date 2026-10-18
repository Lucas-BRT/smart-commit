# Guia de uso

## Instalar

```bash
curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/npu-commit/main/install.sh | bash
```

O instalador:

1. baixa a última release, confere o SHA-256 e instala em `~/.local/bin` e
   `~/.local/share/npu-commit`, sem `sudo`;
2. roda o **setup guiado**, que mostra a NPU e as GPUs encontradas e pergunta
   antes de cada passo que usa `sudo`:
   - NPU Intel: driver da NPU + OpenVINO (`setup-npu.sh`);
   - sem NPU: OpenVINO para rodar na CPU (`setup-npu.sh --openvino-only`);
   - GPUs Intel, AMD e NVIDIA (driver proprietário ou aberto): `setup-gpu.sh`;
3. baixa os modelos, conferindo cada arquivo;
4. roda o **benchmark** e diz qual device será usado:

```text
Benchmark on Intel(R) Core(TM) Ultra 5 225H: 5 prompts, after one warm-up load.

  device   per message     load  generate     CPU
  gpu            1.0 s    0.6 s     0.4 s    1.1x  <- selected
  npu            3.4 s    1.8 s     1.6 s    0.4x
  vulkan         1.6 s    0.8 s     0.8 s    0.5x
  cpu            2.1 s    1.3 s     0.9 s    2.9x

npu-commit will use gpu on this machine (~1.0 s per message): the fastest, model.device = auto.
To use another device: npu-commit device use <npu|gpu|cpu|vulkan>, or `auto` for the fastest.
```

Opções do instalador (depois de `bash -s --` quando vier por `curl`):

| Opção | Efeito |
|---|---|
| `--version 0.2.0` | instala essa versão, não a última |
| `--prefix DIR` | instala em `DIR/bin` e `DIR/share` |
| `--no-setup` | só instala; o setup fica para depois (`~/.local/share/npu-commit/scripts/setup.sh`) |
| `--yes` | responde sim às perguntas do setup |
| `--uninstall` | remove o programa (modelos, cache e config ficam) |

Se `~/.local/bin` não estiver no `PATH`, o instalador avisa. Cada passo do
setup também pode ser rodado sozinho, depois:

```bash
~/.local/share/npu-commit/scripts/setup-npu.sh     # NPU + OpenVINO
~/.local/share/npu-commit/scripts/setup-gpu.sh     # GPUs (--dry-run mostra o plano)
~/.local/share/npu-commit/scripts/fetch-model.sh   # modelo do OpenVINO
npu-commit bench                                   # mede de novo os devices
```

## Uso diário

```bash
git add -p                 # escolha o que vai no commit
npu-commit                 # imprime a mensagem sugerida
npu-commit --commit        # abre o editor com a mensagem e commita
npu-commit -C ../outro     # em outro repositório
```

A mensagem segue o estilo do repositório: a convenção (Conventional
Commits, gitmoji ou livre), o idioma (português ou inglês), os escopos já
usados e o ticket da branch (`ABC-123`), quando o histórico costuma citar.

### Em todo `git commit`, de qualquer cliente

```bash
npu-commit hook install    # no repositório atual
git commit                 # o editor já abre com a mensagem
npu-commit hook uninstall
```

O hook só escreve num `git commit` simples. Com `-m`, `--amend`, merge ou
template, ele não mexe. Se algo falhar, ele avisa e **nunca bloqueia o
commit**.

### Ver o que o modelo recebe

```bash
npu-commit context         # estilo detectado no histórico
npu-commit prompt          # o prompt exato para o que está staged
```

## Devices

```bash
npu-commit device                # qual device está em uso, e os tempos medidos
npu-commit device use npu        # passa a usar a NPU
npu-commit device use auto       # volta para o mais rápido
npu-commit bench                 # mede de novo (depois de instalar drivers, por exemplo)
```

| Device | Backend | Hardware |
|---|---|---|
| `npu` | OpenVINO | NPU Intel (Core Ultra) |
| `gpu` | OpenVINO | GPU Intel (integrada ou Arc) |
| `vulkan` | llama.cpp | qualquer GPU com Vulkan: AMD (RADV), NVIDIA (driver proprietário ou NVK), Intel |
| `cpu` | OpenVINO | qualquer CPU x86_64 |
| `auto` | — | o mais rápido do último `bench`; sem benchmark, o primeiro que funcionar entre gpu, npu, vulkan e cpu |

Com `auto`, se o device escolhido não estiver disponível (por exemplo, um
driver removido), o próximo é usado, com um aviso.

## Configuração

```bash
npu-commit config list --show-origin            # tudo, e de onde cada valor vem
npu-commit config set style.language portuguese
npu-commit config set --repo style.convention conventional   # .npu-commit.toml, vai pro git
npu-commit config init                          # arquivo com todas as opções comentadas
```

O global fica em `~/.config/npu-commit/config.toml`, e o do repositório em
`.npu-commit.toml`. A precedência é: flag > variável de ambiente >
repositório > global > padrão. Todas as chaves estão em
[07 — Configuração](07-configuracao.md).

Receitas comuns:

| Quero… | Comando |
|---|---|
| sempre em português | `npu-commit config set style.language portuguese` |
| forçar Conventional Commits no time | `npu-commit config set --repo style.convention conventional` |
| NPU com resposta mais rápida, vendo menos do diff | `npu-commit config set model.max_prompt_tokens 512` |
| outra GPU (com mais de uma) | `npu-commit config set model.vulkan_device 1` (`llama-server --list-devices`) |
| assunto mais curto, sem corpo longo | `npu-commit config set model.max_new_tokens 64` |

## Reportar uma mensagem ruim

```bash
npu-commit --trace ruim.json     # ou NPU_COMMIT_TRACE=ruim.json git commit
npu-commit replay ruim.json      # confere que se reproduz
```

O trace guarda o diff staged e os assuntos recentes, mas não o caminho do
repositório. Com ele, o caso vira uma fixture de teste
(`tests/fixtures/traces/`). Confira o conteúdo antes de compartilhar.

## Problemas comuns

| Mensagem | O que fazer |
|---|---|
| `no device could run the model` | rode o `setup.sh`; ele diz o que falta para cada device |
| `no model in …` | `~/.local/share/npu-commit/scripts/fetch-model.sh` |
| `no GGUF model at …` / `no llama-server at …` | `~/.local/share/npu-commit/scripts/setup-gpu.sh` |
| `OpenVINO GenAI not found` | `~/.local/share/npu-commit/scripts/setup-npu.sh` (ou `--openvino-only`) |
| `cannot load the model on the NPU` | `~/.local/share/npu-commit/scripts/check-npu.sh`; depois de entrar no grupo `render`, é preciso sair e entrar na sessão |
| NVIDIA: `has no driver bound` | instale o driver (`sudo ubuntu-drivers install`) ou use o nouveau, e rode o `setup-gpu.sh` de novo |
| o hook não faz nada | só age em `git commit` sem `-m`; confira com `npu-commit hook install` (reinstala) |

## Desinstalar

```bash
curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/npu-commit/main/install.sh | bash -s -- --uninstall
rm -rf ~/.local/share/npu-commit ~/.cache/npu-commit ~/.config/npu-commit   # modelos, cache e config
```

Os drivers e o OpenVINO instalados pelo setup continuam no sistema (são
pacotes `.deb` e `/opt/intel`).
