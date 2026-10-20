from dataclasses import dataclass


@dataclass(frozen=True)
class Item:
    codigo: str
    nome: str
    preco: float
    categoria: str


CARDAPIO = {
    "cafe": Item("cafe", "Café coado", 6.00, "bebida"),
    "espresso": Item("espresso", "Espresso", 7.50, "bebida"),
    "capuccino": Item("capuccino", "Capuccino", 11.00, "bebida"),
    "cha": Item("cha", "Chá de camomila", 5.50, "bebida"),
    "pao-de-queijo": Item("pao-de-queijo", "Pão de queijo", 4.50, "salgado"),
    "coxinha": Item("coxinha", "Coxinha", 8.00, "salgado"),
    "bolo": Item("bolo", "Fatia de bolo de cenoura", 9.00, "doce"),
}


def buscar(codigo: str) -> Item:
    try:
        return CARDAPIO[codigo]
    except KeyError:
        raise ValueError(f"item desconhecido: {codigo}") from None


def por_categoria(categoria: str) -> list[Item]:
    return [item for item in CARDAPIO.values() if item.categoria == categoria]
