import sqlite3


class Estoque:
    """Quantidades em estoque, guardadas num sqlite."""

    def __init__(self, caminho: str) -> None:
        self.conexao = sqlite3.connect(caminho)
        self.conexao.execute(
            "create table if not exists estoque (codigo text primary key, quantidade int)"
        )

    def quantidade(self, codigo: str) -> int:
        linha = self.conexao.execute(
            "select quantidade from estoque where codigo = ?", (codigo,)
        ).fetchone()
        return linha[0] if linha else 0

    def repor(self, codigo: str, quantidade: int) -> None:
        self.conexao.execute(
            "insert into estoque values (?, ?) "
            "on conflict(codigo) do update set quantidade = quantidade + excluded.quantidade",
            (codigo, quantidade),
        )
        self.conexao.commit()

    def baixar(self, codigo: str, quantidade: int) -> None:
        atual = self.quantidade(codigo)
        if atual < quantidade:
            raise ValueError(f"estoque insuficente de {codigo}: {atual}")
        self.conexao.execute(
            "update estoque set quantidade = ? where codigo = ?", (atual - quantidade, codigo)
        )
        self.conexao.commit()
