import pytest

from cafeteria.config import Config
from cafeteria.pedido import Pedido


def test_total_inclui_taxa_de_servico():
    pedido = Pedido(mesa=1)
    pedido.adicionar("cafe", 2)
    assert pedido.total(Config()) == 13.2


def test_quantidade_negativa_e_recusada():
    with pytest.raises(ValueError):
        Pedido(mesa=1).adicionar("cafe", -1)


def test_desconto_tem_limite():
    pedido = Pedido(mesa=1, desconto=0.9)
    pedido.adicionar("bolo")
    assert pedido.total(Config()) == round(9 * 1.1 * 0.7, 2)
