//! Testes de integração da linha de comando.
//!
//! Cada arquivo em tests/ é compilado como um crate separado e só
//! enxerga a API pública do crate testado — como reqmeshval é um
//! binário, aqui não dá para importar nada dele. O que se testa é o
//! comportamento externo: argumentos, saída e código de retorno.
//!
//! `CARGO_BIN_EXE_<nome>` é definido pelo próprio Cargo em tempo de
//! compilação dos testes de integração, e aponta para o binário já
//! construído — por isso não é preciso `assert_cmd` nem descobrir o
//! caminho de target/ na mão.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binario() -> &'static str {
    env!("CARGO_BIN_EXE_reqMeshVal")
}

fn fixture(nome: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(nome)
}

fn schema() -> PathBuf {
    fixture("elemento-requisito-v1.toml")
}

fn rodar(args: &[&str]) -> Output {
    Command::new(binario())
        .args(args)
        .output()
        .expect("falha ao executar reqMeshVal")
}

fn stdout(saida: &Output) -> String {
    String::from_utf8_lossy(&saida.stdout).to_string()
}

fn stderr(saida: &Output) -> String {
    String::from_utf8_lossy(&saida.stderr).to_string()
}

#[test]
fn arquivos_validos_saem_com_codigo_zero() {
    let saida = rodar(&[
        schema().to_str().unwrap(),
        fixture("exemplo-pt.toml").to_str().unwrap(),
        fixture("exemplo-en.toml").to_str().unwrap(),
        "--idioma",
        "pt",
    ]);

    assert!(
        saida.status.success(),
        "esperava sucesso; stderr: {}",
        stderr(&saida)
    );

    let texto = stdout(&saida);
    assert!(texto.contains("relatório de validação"));
    assert!(texto.contains("--- resumo ---"));
    assert!(texto.contains("[1/2]"));
    assert!(texto.contains("[2/2]"));
}

#[test]
fn arquivo_com_erro_sai_com_codigo_um() {
    let saida = rodar(&[
        schema().to_str().unwrap(),
        fixture("exemplo-invalido.toml").to_str().unwrap(),
        "--idioma",
        "pt",
    ]);

    assert_eq!(saida.status.code(), Some(1));
    assert!(stdout(&saida).contains("FALHA"));
}

#[test]
fn schema_inexistente_sai_com_codigo_dois() {
    let saida = rodar(&[
        "/nao/existe/schema.toml",
        fixture("exemplo-pt.toml").to_str().unwrap(),
    ]);

    assert_eq!(saida.status.code(), Some(2));
}

#[test]
fn sem_argumentos_suficientes_sai_com_codigo_dois() {
    let saida = rodar(&[schema().to_str().unwrap()]);
    assert_eq!(saida.status.code(), Some(2));
    // A ajuda vai para stderr junto com o erro de uso.
    assert!(stderr(&saida).contains("reqMeshVal"));
}

#[test]
fn idioma_altera_a_saida() {
    let args_pt = [
        schema().to_str().unwrap().to_string(),
        fixture("exemplo-pt.toml").to_str().unwrap().to_string(),
        "--idioma".into(),
        "pt".into(),
    ];
    let args_en = [
        schema().to_str().unwrap().to_string(),
        fixture("exemplo-pt.toml").to_str().unwrap().to_string(),
        "--idioma".into(),
        "en".into(),
    ];

    let pt = rodar(&args_pt.iter().map(String::as_str).collect::<Vec<_>>());
    let en = rodar(&args_en.iter().map(String::as_str).collect::<Vec<_>>());

    assert!(stdout(&pt).contains("relatório de validação"));
    assert!(stdout(&en).contains("validation report"));
    // O mesmo arquivo, o mesmo veredito — só o idioma muda.
    assert_eq!(pt.status.code(), en.status.code());
}

/// No modo auto cada arquivo é reportado no idioma que ele próprio
/// declara, então uma única execução mistura os dois.
#[test]
fn modo_auto_reporta_cada_arquivo_no_idioma_declarado() {
    let saida = rodar(&[
        schema().to_str().unwrap(),
        fixture("exemplo-pt.toml").to_str().unwrap(),
        fixture("exemplo-en.toml").to_str().unwrap(),
        "--idioma",
        "auto",
    ]);

    let texto = stdout(&saida);
    assert!(
        texto.contains("campos reconhecidos"),
        "esperava o bloco em português"
    );
    assert!(
        texto.contains("fields recognized"),
        "esperava o bloco em inglês"
    );
}

#[test]
fn idioma_desconhecido_sai_com_codigo_dois() {
    let saida = rodar(&[
        schema().to_str().unwrap(),
        fixture("exemplo-pt.toml").to_str().unwrap(),
        "--idioma",
        "tlh",
    ]);

    assert_eq!(saida.status.code(), Some(2));
}

#[test]
fn listar_idiomas_mostra_os_embutidos() {
    let saida = rodar(&["--idiomas"]);
    assert!(saida.status.success());
    let texto = stdout(&saida);
    assert!(texto.contains("en"));
    assert!(texto.contains("pt"));
}

#[test]
fn ajuda_sai_com_sucesso() {
    for flag in ["-h", "--help"] {
        let saida = rodar(&[flag]);
        assert!(saida.status.success());
        assert!(stdout(&saida).contains("reqMeshVal"));
    }
}

#[test]
fn opcao_log_grava_o_relatorio_em_arquivo() {
    let destino = std::env::temp_dir().join(format!("reqmeshval-teste-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&destino);

    let saida = rodar(&[
        schema().to_str().unwrap(),
        fixture("exemplo-pt.toml").to_str().unwrap(),
        "--idioma",
        "pt",
        "--log",
        destino.to_str().unwrap(),
    ]);

    assert!(saida.status.success());

    let gravado = std::fs::read_to_string(&destino).expect("log não foi gravado");
    // O arquivo precisa conter o mesmo relatório impresso na saída.
    assert_eq!(gravado, stdout(&saida));

    let _ = std::fs::remove_file(&destino);
}
