from dataclasses import dataclass


@dataclass
class Config:
    nome_loja: str = "Café da Esquina"
    moeda: str = "R$"
    taxa_servico: float = 0.10
    desconto_maximo: float = 0.30
    arquivo_estoque: str = "estoque.sqlite"
