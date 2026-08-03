//! Casca do executável: repassa os argumentos (sem o `argv[0]`) para a
//! biblioteca e devolve o código de saída. Toda a lógica — inclusive o
//! parsing da linha de comando — vive em `reqmeshval`, para que os testes
//! de integração em `tests/` alcancem o mesmo caminho que o binário.

use std::process::ExitCode;

fn main() -> ExitCode {
    reqmeshval::executar(std::env::args().skip(1))
}
