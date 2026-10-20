from dataclasses import dataclass, field

from cafeteria.cardapio import Item, buscar
from cafeteria.config import Config


@dataclass
class Linha:
    item: Item
    quantidade: int = 1

    @property
    def subtotal(self) -> float:
        return self.item.preco * self.quantidade


@dataclass
class Pedido:
    mesa: int
    linhas: list[Linha] = field(default_factory=list)
    desconto: float = 0.0

    def adicionar(self, codigo: str, quantidade: int = 1) -> None:
        if quantidade <= 0:
            raise ValueError("quantidade deve ser positiva")
        for linha in self.linhas:
            if linha.item.codigo == codigo:
                linha.quantidade += quantidade
                return
        self.linhas.append(Linha(buscar(codigo), quantidade))

    def remover(self, codigo: str) -> None:
        self.linhas = [l for l in self.linhas if l.item.codigo != codigo]

    def subtotal(self) -> float:
        return sum(linha.subtotal for linha in self.linhas)

    def total(self, config: Config) -> float:
        bruto = self.subtotal() * (1 + config.taxa_servico)
        desconto = min(self.desconto, config.desconto_maximo)
        return round(bruto * (1 - desconto), 2)
