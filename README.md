# npu-commit

Gera mensagens de commit no estilo do seu repositório usando um LLM pequeno
rodando **localmente na NPU Intel** (OpenVINO). Nada sai da sua máquina.

> Status: fases 1 e 2 concluídas (contexto do repositório, prompt e saneamento). Ainda não gera mensagens.

```bash
cargo run -- context [PATH]   # mostra o que foi detectado no repositório
cargo run -- prompt [PATH]    # mostra o prompt que o modelo receberia
cargo test                    # não precisa de NPU
```

## Documentação

- [01 — Design](docs/01-design.md)
- [02 — Fase 1: contexto do repositório](docs/02-fase-1-contexto.md)
- [03 — Fase 2: diff, prompt e saneamento](docs/03-fase-2-prompt.md)
