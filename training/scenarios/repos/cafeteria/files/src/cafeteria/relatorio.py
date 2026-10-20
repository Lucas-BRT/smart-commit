from collections import Counter
from pathlib import Path

from tabulate import tabulate

from cafeteria.config import Config
from cafeteria.pedido import Pedido

TEMPLATES = Path(__file__).parent.parent.parent / "templates"


def resumo_do_dia(pedidos: list[Pedido], config: Config) -> str:
    vendidos: Counter[str] = Counter()
    for pedido in pedidos:
        for linha in pedido.linhas:
            vendidos[linha.item.nome] += linha.quantidade
    total = sum(pedido.total(config) for pedido in pedidos)
    tabela = tabulate(vendidos.most_common(), headers=["item", "qtd"])
    return f"{tabela}\n\ntotal: {config.moeda} {total:.2f}"


def recibo(pedido: Pedido, config: Config) -> str:
    modelo = (TEMPLATES / "recibo.txt").read_text()
    linhas = "\n".join(
        f"{l.quantidade}x {l.item.nome:<24} {config.moeda} {l.subtotal:>6.2f}" for l in pedido.linhas
    )
    return modelo.format(
        loja=config.nome_loja, mesa=pedido.mesa, linhas=linhas,
        total=f"{config.moeda} {pedido.total(config):.2f}",
    )
