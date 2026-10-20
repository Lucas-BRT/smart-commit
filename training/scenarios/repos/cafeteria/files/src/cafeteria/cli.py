import argparse
import sys

from cafeteria.config import Config
from cafeteria.pedido import Pedido
from cafeteria.relatorio import recibo


def main() -> None:
    parser = argparse.ArgumentParser(prog="cafeteria")
    sub = parser.add_subparsers(dest="comando", required=True)
    pedido = sub.add_parser("pedido", help="registra um pedido e imprime o recibo")
    pedido.add_argument("itens", nargs="+")
    pedido.add_argument("--mesa", type=int, default=1)
    pedido.add_argument("--desconto", type=float, default=0.0)
    sub.add_parser("relatorio", help="mostra o resumo do dia")  
    args = parser.parse_args()

    config = Config()  
    if args.comando == "pedido":
        novo = Pedido(mesa=args.mesa, desconto=args.desconto)  
        for codigo in args.itens:
            novo.adicionar(codigo)
        print(recibo(novo, config))
    else:
        print("nenhum pedido registrado hoje")


if __name__ == "__main__":
    main()
