//! Testes de integração: percorrem o mesmo pipeline que o binário
//! (`parse_cli → carregar_schema → executar_validacao → montar_log`)
//! usando apenas a API pública da lib. Só passaram a ser possíveis depois
//! da separação `lib.rs` + `main.rs`: um crate binário não é importável
//! de `tests/`.

use std::path::{Path, PathBuf};

use reqmeshval::{
    carregar_schema, executar_validacao, montar_log, parse_cli, Acao, Idiomas,
};

fn fixture(nome: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(nome)
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// A cola inteira, do argv ao texto do log, sem passar pelo processo.
#[test]
fn pipeline_completo_em_pt() {
    let idiomas = Idiomas::carregar(None);
    let schema = fixture("elemento-requisito-v1.toml");
    let alvo = fixture("exemplo-pt.toml");

    let acao = parse_cli(
        &args(&[
            "-i",
            "pt",
            schema.to_str().unwrap(),
            alvo.to_str().unwrap(),
        ]),
        &idiomas,
        "en",
    )
    .expect("linha de comando válida");

    let cfg = match acao {
        Acao::Validar(cfg) => cfg,
        outro => panic!("esperava Validar, obtive {outro:?}"),
    };

    let (schema, versao) = carregar_schema(&cfg.caminho_schema).expect("schema carrega");
    let resultados = executar_validacao(&schema, &versao, &cfg.arquivos);

    assert_eq!(resultados.len(), 1);
    let (_, rel) = &resultados[0];
    assert!(rel.valido(), "diagnósticos: {:?}", rel.chaves());
    assert_eq!(rel.tipo.as_deref(), Some("caso_de_uso"));

    let log = montar_log(&cfg, &resultados, &idiomas);
    assert!(log.contains(&idiomas.rot("pt", "ok")));
    assert!(!log.contains('⟨'));
}

/// Arquivo com defeitos precisa chegar ao log como falha, e o log não pode
/// conter chave de catálogo não resolvida em nenhum idioma disponível.
#[test]
fn arquivo_invalido_vira_falha_em_qualquer_idioma() {
    let idiomas = Idiomas::carregar(None);
    let schema_path = fixture("elemento-requisito-v1.toml");
    let alvo = fixture("exemplo-invalido.toml");

    for codigo in idiomas.disponiveis() {
        let acao = parse_cli(
            &args(&[
                "-i",
                &codigo,
                schema_path.to_str().unwrap(),
                alvo.to_str().unwrap(),
            ]),
            &idiomas,
            "en",
        )
        .expect("linha de comando válida");

        let cfg = match acao {
            Acao::Validar(cfg) => cfg,
            outro => panic!("esperava Validar, obtive {outro:?}"),
        };

        let (schema, versao) = carregar_schema(&cfg.caminho_schema).expect("schema carrega");
        let resultados = executar_validacao(&schema, &versao, &cfg.arquivos);
        assert!(!resultados[0].1.valido());

        let log = montar_log(&cfg, &resultados, &idiomas);
        assert!(log.contains(&idiomas.rot(&codigo, "falha")), "idioma {codigo}");
        assert!(!log.contains('⟨'), "chave ausente em '{codigo}':\n{log}");
    }
}
