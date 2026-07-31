//! reqMeshVal — validador de Elementos de Requisito (ReqMesh).
//!
//! Uso:
//!     reqMeshVal <schema> <arquivo1> [arquivo2 ...] [opções]
//!
//! As mensagens vivem em catálogos TOML (i18n/<codigo>.toml), não no
//! código. Os catálogos de `en` e `pt` são embutidos no binário em tempo
//! de compilação; `--catalogos <dir>` carrega (ou sobrepõe) outros a
//! partir do disco, então adicionar um idioma não exige recompilar.
//!
//! Os diagnósticos são acumulados como dados (`Msg`), não como texto já
//! formatado: cada variante vira uma chave de catálogo mais argumentos
//! nomeados, e o idioma só é aplicado na renderização. É isso que
//! permite `--idioma auto` (cada arquivo reportado no idioma que ele
//! próprio declara) sem revalidar nada.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use toml::Value;

/// Único campo fixo do Elemento de Requisito, nunca localizado.
const CAMPO_SCHEMA_ID: &str = "schema_id";
/// Idioma de conteúdo assumido quando o locale vem vazio ou ausente.
const IDIOMA_PADRAO_CONTEUDO: &str = "en";
/// Nome canônico do campo discriminador de tipo do elemento.
const CAMPO_TIPO: &str = "tipo";
/// Idioma usado como fallback quando falta chave ou catálogo.
const FALLBACK: &str = "en";

/// Catálogos embutidos no binário. Adicionar um idioma ao repositório é
/// acrescentar o arquivo e uma linha aqui — ou simplesmente entregá-lo
/// via `--catalogos`, sem tocar no código.
const EMBUTIDOS: &[(&str, &str)] = &[
    ("en", include_str!("../i18n/en.toml")),
    ("pt", include_str!("../i18n/pt.toml")),
];

// ============================================================
// Catálogo de mensagens
// ============================================================

#[derive(Debug, Deserialize, Default)]
struct Catalogo {
    #[serde(default)]
    ui: BTreeMap<String, String>,
    #[serde(default)]
    msg: BTreeMap<String, String>,
    #[serde(default)]
    plural: BTreeMap<String, FormasPlural>,
}

#[derive(Debug, Deserialize)]
struct FormasPlural {
    one: String,
    other: String,
}

#[derive(Debug, Clone, Copy)]
enum Secao {
    Ui,
    Msg,
}

/// Conjunto de catálogos carregados, indexado pelo código do idioma.
#[derive(Debug, Default)]
struct Idiomas {
    catalogos: BTreeMap<String, Catalogo>,
}

impl Idiomas {
    /// Carrega os catálogos embutidos e, se informado, os do diretório
    /// (que sobrepõem os embutidos de mesmo código).
    fn carregar(dir: Option<&Path>) -> Idiomas {
        let mut catalogos: BTreeMap<String, Catalogo> = BTreeMap::new();

        for (codigo, texto) in EMBUTIDOS {
            match toml::from_str::<Catalogo>(texto) {
                Ok(c) => {
                    catalogos.insert((*codigo).to_string(), c);
                }
                // Catálogo embutido quebrado é erro de build, não do usuário.
                Err(e) => eprintln!("reqMeshVal: catálogo embutido '{codigo}' inválido: {e}"),
            }
        }

        let mut idiomas = Idiomas { catalogos };

        if let Some(dir) = dir {
            idiomas.carregar_dir(dir);
        }

        idiomas
    }

    fn carregar_dir(&mut self, dir: &Path) {
        let entradas = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("reqMeshVal: {}: {e}", dir.display());
                return;
            }
        };

        for entrada in entradas.flatten() {
            let caminho = entrada.path();
            if caminho.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let codigo = match caminho.file_stem().and_then(|s| s.to_str()) {
                Some(c) => c.to_lowercase(),
                None => continue,
            };

            let conteudo = match fs::read_to_string(&caminho) {
                Ok(c) => c,
                Err(e) => {
                    self.avisar_catalogo(&caminho, &e.to_string());
                    continue;
                }
            };

            match toml::from_str::<Catalogo>(&conteudo) {
                Ok(c) => {
                    self.catalogos.insert(codigo, c);
                }
                Err(e) => self.avisar_catalogo(&caminho, &e.to_string()),
            }
        }
    }

    /// Aviso emitido antes de o idioma da sessão estar decidido; usa o
    /// fallback por necessidade.
    fn avisar_catalogo(&self, caminho: &Path, erro: &str) {
        eprintln!(
            "{}",
            self.ui(
                FALLBACK,
                "catalogo-invalido",
                &[
                    ("caminho", caminho.display().to_string()),
                    ("erro", erro.to_string()),
                ],
            )
        );
    }

    fn tem(&self, codigo: &str) -> bool {
        self.catalogos.contains_key(codigo)
    }

    fn disponiveis(&self) -> Vec<String> {
        self.catalogos.keys().cloned().collect()
    }

    /// Resolve o idioma efetivo: o pedido, se existir catálogo; senão o
    /// fallback.
    fn efetivo(&self, codigo: &str) -> String {
        if self.tem(codigo) {
            codigo.to_string()
        } else {
            FALLBACK.to_string()
        }
    }

    fn texto(&self, codigo: &str, secao: Secao, chave: &str) -> String {
        for tentativa in [codigo, FALLBACK] {
            if let Some(cat) = self.catalogos.get(tentativa) {
                let mapa = match secao {
                    Secao::Ui => &cat.ui,
                    Secao::Msg => &cat.msg,
                };
                if let Some(s) = mapa.get(chave) {
                    return s.clone();
                }
            }
        }
        // Chave inexistente aparece na saída em vez de sumir silenciosamente.
        format!("⟨{chave}⟩")
    }

    fn ui(&self, codigo: &str, chave: &str, args: &[(&str, String)]) -> String {
        interpolar(&self.texto(codigo, Secao::Ui, chave), args)
    }

    /// Atalho para rótulos sem argumentos.
    fn rot(&self, codigo: &str, chave: &str) -> String {
        self.texto(codigo, Secao::Ui, chave)
    }

    fn msg(&self, codigo: &str, chave: &str, args: &[(&str, String)]) -> String {
        interpolar(&self.texto(codigo, Secao::Msg, chave), args)
    }

    /// Seleção de forma plural. Regra simples: 1 -> `one`, resto -> `other`.
    /// Cobre pt e en; idiomas com regras mais ricas (russo, polonês, árabe)
    /// precisariam de categorias adicionais no catálogo.
    fn plural(&self, codigo: &str, chave: &str, n: usize) -> String {
        for tentativa in [codigo, FALLBACK] {
            if let Some(cat) = self.catalogos.get(tentativa) {
                if let Some(formas) = cat.plural.get(chave) {
                    let gabarito = if n == 1 { &formas.one } else { &formas.other };
                    return interpolar(gabarito, &[("n", n.to_string())]);
                }
            }
        }
        format!("⟨{chave}:{n}⟩")
    }
}

/// Substitui `{nome}` pelos argumentos fornecidos. Placeholder não
/// fornecido permanece literal — falha visível, não silenciosa.
fn interpolar(gabarito: &str, args: &[(&str, String)]) -> String {
    let mut s = gabarito.to_string();
    for (chave, valor) in args {
        s = s.replace(&format!("{{{chave}}}"), valor);
    }
    s
}

fn idioma_do_ambiente() -> Option<String> {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(valor) = std::env::var(var) {
            let base = valor
                .split(['_', '.', '-'])
                .next()
                .unwrap_or("")
                .to_lowercase();
            if !base.is_empty() && base != "c" && base != "posix" {
                return Some(base);
            }
        }
    }
    None
}

// ============================================================
// Modelo do arquivo de schema
// ============================================================

#[derive(Debug, Deserialize)]
struct Schema {
    #[serde(default)]
    campos: BTreeMap<String, DefCampo>,
    #[serde(default)]
    campos_caso_de_uso: BTreeMap<String, DefCampo>,
    #[serde(default)]
    campos_ator: BTreeMap<String, DefCampo>,
}

#[derive(Debug, Clone, Deserialize)]
struct DefCampo {
    #[serde(default)]
    obrigatorio: bool,
    tipo_dado: String,
    /// idioma -> nome do campo naquele idioma
    #[serde(default)]
    alias: BTreeMap<String, String>,
    /// valor canônico -> aliases por idioma (só para tipo_dado = "enum")
    #[serde(default)]
    valores: BTreeMap<String, DefValor>,
    /// só para tipo_dado = "array<tabela>"
    #[serde(default)]
    subcampos: BTreeMap<String, DefCampo>,
}

#[derive(Debug, Clone, Deserialize)]
struct DefValor {
    #[serde(default)]
    alias: BTreeMap<String, String>,
}

// ============================================================
// Diagnósticos (dados, não texto)
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nivel {
    Erro,
    Aviso,
}

impl Nivel {
    fn chave(self) -> &'static str {
        match self {
            Nivel::Erro => "nivel-erro",
            Nivel::Aviso => "nivel-aviso",
        }
    }
}

/// Onde o problema foi encontrado.
#[derive(Debug, Clone)]
enum Local {
    Arquivo,
    Toml,
    Schema,
    /// Caminho do campo como escrito no arquivo (ex: "relacoes[0].tipo").
    /// Não é traduzido: é o que o autor vai procurar no arquivo dele.
    Campo(String),
}

impl Local {
    fn render(&self, idiomas: &Idiomas, codigo: &str) -> String {
        match self {
            Local::Arquivo => idiomas.rot(codigo, "local-arquivo"),
            Local::Toml => idiomas.rot(codigo, "local-toml"),
            Local::Schema => idiomas.rot(codigo, "local-schema"),
            Local::Campo(c) => c.clone(),
        }
    }
}

/// Toda mensagem que o validador sabe emitir. Guardar os dados em vez do
/// texto é o que permite renderizar o mesmo diagnóstico em qualquer idioma.
#[derive(Debug, Clone)]
enum Msg {
    ArquivoIlegivel(String),
    TomlInvalido(String),
    NaoEhTabelaRaiz,
    SchemaIdAusente,
    VersaoDivergente {
        no_arquivo: String,
        carregado: String,
    },
    SchemaSemCampoCanonico(String),
    SchemaSemAliasDoCampo {
        campo: String,
        idioma: String,
    },
    TipoAusente,
    TipoSemCamposEspecificos(String),
    EsperadoTexto(String),
    EsperadoListaTextos(String),
    EsperadoListaTabelas(String),
    EsperadoTabela(String),
    ListaVaziaObrigatoria,
    EnumInvalido {
        valor: String,
        idioma: String,
        permitidos: Vec<String>,
        em_outro_idioma: Option<(String, String)>,
    },
    AliasDeOutroIdioma {
        canonico: String,
        idioma_do_alias: String,
        idioma_declarado: String,
        subcampo: bool,
    },
    CampoDesconhecido {
        idioma: String,
        subcampo: bool,
    },
    ObrigatorioAusente {
        canonico: String,
        subcampo: bool,
    },
    TipoDadoDesconhecido(String),
    SemSubcampos,
    AliasAmbiguo {
        alias: String,
        idioma: String,
        anterior: String,
        atual: String,
    },
    SemAliasNoIdioma {
        canonico: String,
        idioma: String,
    },
}

impl Msg {
    /// Chave do catálogo e argumentos nomeados. Nenhum texto de usuário
    /// aparece aqui — só dados.
    fn chave_e_args(&self) -> (&'static str, Vec<(&'static str, String)>) {
        match self {
            Msg::ArquivoIlegivel(e) => ("arquivo-ilegivel", vec![("erro", e.clone())]),
            Msg::TomlInvalido(e) => ("toml-invalido", vec![("erro", e.clone())]),
            Msg::NaoEhTabelaRaiz => ("nao-eh-tabela-raiz", vec![]),
            Msg::SchemaIdAusente => (
                "schema-id-ausente",
                vec![("campo", CAMPO_SCHEMA_ID.to_string())],
            ),
            Msg::VersaoDivergente {
                no_arquivo,
                carregado,
            } => (
                "versao-divergente",
                vec![
                    ("no_arquivo", no_arquivo.clone()),
                    ("carregado", carregado.clone()),
                ],
            ),
            Msg::SchemaSemCampoCanonico(c) => {
                ("schema-sem-campo-canonico", vec![("campo", c.clone())])
            }
            Msg::SchemaSemAliasDoCampo { campo, idioma } => (
                "schema-sem-alias-do-campo",
                vec![("campo", campo.clone()), ("idioma", idioma.clone())],
            ),
            Msg::TipoAusente => ("tipo-ausente", vec![]),
            Msg::TipoSemCamposEspecificos(t) => {
                ("tipo-sem-campos-especificos", vec![("tipo", t.clone())])
            }
            Msg::EsperadoTexto(enc) => ("esperado-texto", vec![("encontrado", enc.clone())]),
            Msg::EsperadoListaTextos(enc) => {
                ("esperado-lista-textos", vec![("encontrado", enc.clone())])
            }
            Msg::EsperadoListaTabelas(enc) => {
                ("esperado-lista-tabelas", vec![("encontrado", enc.clone())])
            }
            Msg::EsperadoTabela(enc) => ("esperado-tabela", vec![("encontrado", enc.clone())]),
            Msg::ListaVaziaObrigatoria => ("lista-vazia-obrigatoria", vec![]),
            Msg::EnumInvalido {
                valor,
                idioma,
                permitidos,
                em_outro_idioma,
            } => {
                let mut args = vec![
                    ("valor", valor.clone()),
                    ("idioma", idioma.clone()),
                    ("permitidos", permitidos.join(", ")),
                ];
                match em_outro_idioma {
                    Some((canonico, lang)) => {
                        args.push(("canonico", canonico.clone()));
                        args.push(("idioma_do_alias", lang.clone()));
                        ("enum-invalido-outro-idioma", args)
                    }
                    None => ("enum-invalido", args),
                }
            }
            Msg::AliasDeOutroIdioma {
                canonico,
                idioma_do_alias,
                idioma_declarado,
                subcampo,
            } => {
                let chave = if *subcampo {
                    "alias-de-outro-idioma-subcampo"
                } else {
                    "alias-de-outro-idioma"
                };
                (
                    chave,
                    vec![
                        ("canonico", canonico.clone()),
                        ("idioma_do_alias", idioma_do_alias.clone()),
                        ("idioma_declarado", idioma_declarado.clone()),
                    ],
                )
            }
            Msg::CampoDesconhecido { idioma, subcampo } => {
                let chave = if *subcampo {
                    "subcampo-desconhecido"
                } else {
                    "campo-desconhecido"
                };
                (chave, vec![("idioma", idioma.clone())])
            }
            Msg::ObrigatorioAusente {
                canonico,
                subcampo,
            } => {
                let chave = if *subcampo {
                    "subcampo-obrigatorio-ausente"
                } else {
                    "obrigatorio-ausente"
                };
                (chave, vec![("canonico", canonico.clone())])
            }
            Msg::TipoDadoDesconhecido(t) => {
                ("tipo-dado-desconhecido", vec![("tipo_dado", t.clone())])
            }
            Msg::SemSubcampos => ("sem-subcampos", vec![]),
            Msg::AliasAmbiguo {
                alias,
                idioma,
                anterior,
                atual,
            } => (
                "alias-ambiguo",
                vec![
                    ("alias", alias.clone()),
                    ("idioma", idioma.clone()),
                    ("anterior", anterior.clone()),
                    ("atual", atual.clone()),
                ],
            ),
            Msg::SemAliasNoIdioma { canonico, idioma } => (
                "sem-alias-no-idioma",
                vec![("canonico", canonico.clone()), ("idioma", idioma.clone())],
            ),
        }
    }

    fn render(&self, idiomas: &Idiomas, codigo: &str) -> String {
        let (chave, args) = self.chave_e_args();
        idiomas.msg(codigo, chave, &args)
    }
}

#[derive(Debug)]
struct Diagnostico {
    nivel: Nivel,
    local: Local,
    msg: Msg,
}

impl Diagnostico {
    fn erro(local: Local, msg: Msg) -> Self {
        Diagnostico {
            nivel: Nivel::Erro,
            local,
            msg,
        }
    }

    fn aviso(local: Local, msg: Msg) -> Self {
        Diagnostico {
            nivel: Nivel::Aviso,
            local,
            msg,
        }
    }
}

fn campo(nome: impl Into<String>) -> Local {
    Local::Campo(nome.into())
}

#[derive(Debug, Default)]
struct Relatorio {
    schema_id: Option<String>,
    idioma: Option<String>,
    tipo: Option<String>,
    campos_reconhecidos: usize,
    diagnosticos: Vec<Diagnostico>,
}

impl Relatorio {
    fn erros(&self) -> usize {
        self.diagnosticos
            .iter()
            .filter(|d| d.nivel == Nivel::Erro)
            .count()
    }

    fn avisos(&self) -> usize {
        self.diagnosticos
            .iter()
            .filter(|d| d.nivel == Nivel::Aviso)
            .count()
    }

    fn valido(&self) -> bool {
        self.erros() == 0
    }
}

// ============================================================
// main / CLI
// ============================================================

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // O diretório de catálogos precisa ser lido antes de qualquer mensagem,
    // então é a única opção varrida numa passagem prévia.
    let mut dir_catalogos: Option<PathBuf> = None;
    for (i, a) in args.iter().enumerate() {
        if (a == "-c" || a == "--catalogos") && i + 1 < args.len() {
            dir_catalogos = Some(PathBuf::from(&args[i + 1]));
        }
    }

    let idiomas = Idiomas::carregar(dir_catalogos.as_deref());
    let padrao = idiomas.efetivo(&idioma_do_ambiente().unwrap_or_default());

    let mut posicionais: Vec<String> = Vec::new();
    let mut caminho_log: Option<PathBuf> = None;
    // None = seguir o padrão; Some(None) = auto; Some(Some(cod)) = fixo.
    let mut idioma_pedido: Option<Option<String>> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("{}", idiomas.rot(&padrao, "uso"));
                return ExitCode::SUCCESS;
            }
            "--idiomas" => {
                println!("{}", idiomas.disponiveis().join("\n"));
                return ExitCode::SUCCESS;
            }
            "-c" | "--catalogos" => {
                i += 1; // já tratado na passagem prévia
            }
            "-l" | "--log" => {
                i += 1;
                match args.get(i) {
                    Some(v) => caminho_log = Some(PathBuf::from(v)),
                    None => {
                        eprintln!(
                            "{}\n\n{}",
                            idiomas.rot(&padrao, "cli-log-sem-caminho"),
                            idiomas.rot(&padrao, "uso")
                        );
                        return ExitCode::from(2);
                    }
                }
            }
            "-i" | "--idioma" | "--lang" => {
                i += 1;
                match args.get(i) {
                    Some(v) if v.eq_ignore_ascii_case("auto") => idioma_pedido = Some(None),
                    Some(v) => {
                        let cod = v.to_lowercase();
                        if !idiomas.tem(&cod) {
                            eprintln!(
                                "{}",
                                idiomas.ui(
                                    &padrao,
                                    "cli-idioma-invalido",
                                    &[
                                        ("idioma", cod),
                                        ("disponiveis", idiomas.disponiveis().join(", ")),
                                    ],
                                )
                            );
                            return ExitCode::from(2);
                        }
                        idioma_pedido = Some(Some(cod));
                    }
                    None => {
                        eprintln!("{}", idiomas.rot(&padrao, "cli-idioma-sem-valor"));
                        return ExitCode::from(2);
                    }
                }
            }
            outro => posicionais.push(outro.to_string()),
        }
        i += 1;
    }

    let auto = matches!(idioma_pedido, Some(None));
    // Idioma do cabeçalho, do resumo e das mensagens fora de arquivo.
    let l_geral = match &idioma_pedido {
        Some(Some(cod)) => cod.clone(),
        _ => padrao.clone(),
    };

    if posicionais.len() < 2 {
        eprintln!(
            "{}\n\n{}",
            idiomas.rot(&l_geral, "cli-faltam-args"),
            idiomas.rot(&l_geral, "uso")
        );
        return ExitCode::from(2);
    }

    let caminho_schema = PathBuf::from(&posicionais[0]);
    let arquivos: Vec<PathBuf> = posicionais[1..].iter().map(PathBuf::from).collect();

    // --- carrega o schema (falha aqui é fatal) ---
    let bruto_schema = match fs::read_to_string(&caminho_schema) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{}",
                idiomas.ui(
                    &l_geral,
                    "schema-ilegivel",
                    &[
                        ("caminho", caminho_schema.display().to_string()),
                        ("erro", e.to_string()),
                    ],
                )
            );
            return ExitCode::from(2);
        }
    };

    let schema: Schema = match toml::from_str(&bruto_schema) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "{}",
                idiomas.ui(
                    &l_geral,
                    "schema-invalido",
                    &[
                        ("caminho", caminho_schema.display().to_string()),
                        ("erro", e.to_string()),
                    ],
                )
            );
            return ExitCode::from(2);
        }
    };

    let versao_esperada = caminho_schema
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let descricao_idioma = if auto {
        "auto".to_string()
    } else {
        l_geral.clone()
    };

    // --- cabeçalho ---
    let mut log = String::new();
    log.push_str(&format!("{}\n", idiomas.rot(&l_geral, "titulo")));
    log.push_str(&format!(
        "{:<9}: {}\n",
        idiomas.rot(&l_geral, "data"),
        chrono::Local::now().to_rfc3339()
    ));
    log.push_str(&format!(
        "{:<9}: {}\n",
        idiomas.rot(&l_geral, "schema"),
        caminho_schema.display()
    ));
    log.push_str(&format!(
        "{:<9}: {versao_esperada}\n",
        idiomas.rot(&l_geral, "versao")
    ));
    log.push_str(&format!(
        "{:<9}: {}\n",
        idiomas.rot(&l_geral, "arquivos"),
        arquivos.len()
    ));
    log.push_str(&format!(
        "{:<9}: {descricao_idioma}\n\n",
        idiomas.rot(&l_geral, "idioma")
    ));

    // --- validação arquivo por arquivo ---
    let total = arquivos.len();
    let mut validos = 0usize;
    let mut total_erros = 0usize;
    let mut total_avisos = 0usize;

    for (idx, arquivo) in arquivos.iter().enumerate() {
        let rel = validar_arquivo(&schema, &versao_esperada, arquivo);

        if rel.valido() {
            validos += 1;
        }
        total_erros += rel.erros();
        total_avisos += rel.avisos();

        // No modo auto, cada arquivo é reportado no idioma que ele declara;
        // idioma sem catálogo cai no fallback.
        let l_arquivo = if auto {
            idiomas.efetivo(rel.idioma.as_deref().unwrap_or(&padrao))
        } else {
            l_geral.clone()
        };

        log.push_str(&formatar_arquivo(
            idx + 1,
            total,
            arquivo,
            &rel,
            &idiomas,
            &l_arquivo,
        ));
    }

    // --- resumo ---
    log.push_str(&format!("{}\n", idiomas.rot(&l_geral, "resumo")));
    for (chave, valor) in [
        ("arquivos", total),
        ("validos", validos),
        ("falhas", total - validos),
        ("erros", total_erros),
        ("avisos", total_avisos),
    ] {
        log.push_str(&format!(
            "{:<9}: {valor}\n",
            idiomas.rot(&l_geral, chave)
        ));
    }

    print!("{log}");

    if let Some(caminho) = &caminho_log {
        if let Err(e) = fs::write(caminho, &log) {
            eprintln!(
                "{}",
                idiomas.ui(
                    &l_geral,
                    "log-nao-gravado",
                    &[
                        ("caminho", caminho.display().to_string()),
                        ("erro", e.to_string()),
                    ],
                )
            );
        }
    }

    if total_erros > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn formatar_arquivo(
    indice: usize,
    total: usize,
    caminho: &Path,
    rel: &Relatorio,
    idiomas: &Idiomas,
    l: &str,
) -> String {
    let mut s = String::new();
    s.push_str(&format!("[{indice}/{total}] {}\n", caminho.display()));

    if let Some(sid) = &rel.schema_id {
        let idioma_arq = rel.idioma.as_deref().unwrap_or("?");
        s.push_str(&format!(
            "         {CAMPO_SCHEMA_ID} : {sid}  ({}: {idioma_arq})\n",
            idiomas.rot(l, "idioma")
        ));
    }
    if let Some(tipo) = &rel.tipo {
        s.push_str(&format!("         {:<9} : {tipo}\n", idiomas.rot(l, "tipo")));
    }

    if rel.valido() {
        s.push_str(&format!(
            "         {} — {}, {}\n",
            idiomas.rot(l, "ok"),
            idiomas.plural(l, "campos-reconhecidos", rel.campos_reconhecidos),
            idiomas.plural(l, "avisos", rel.avisos()),
        ));
    } else {
        s.push_str(&format!(
            "         {} — {}, {}\n",
            idiomas.rot(l, "falha"),
            idiomas.plural(l, "erros", rel.erros()),
            idiomas.plural(l, "avisos", rel.avisos()),
        ));
    }

    for d in &rel.diagnosticos {
        s.push_str(&format!(
            "           {:<5} {:<28} {}\n",
            idiomas.rot(l, d.nivel.chave()),
            d.local.render(idiomas, l),
            d.msg.render(idiomas, l)
        ));
    }

    s.push('\n');
    s
}

// ============================================================
// Validação de um arquivo
// ============================================================

fn validar_arquivo(schema: &Schema, versao_esperada: &str, caminho: &Path) -> Relatorio {
    let mut rel = Relatorio::default();

    let conteudo = match fs::read_to_string(caminho) {
        Ok(c) => c,
        Err(e) => {
            rel.diagnosticos.push(Diagnostico::erro(
                Local::Arquivo,
                Msg::ArquivoIlegivel(e.to_string()),
            ));
            return rel;
        }
    };

    let valor: Value = match toml::from_str(&conteudo) {
        Ok(v) => v,
        Err(e) => {
            rel.diagnosticos.push(Diagnostico::erro(
                Local::Toml,
                Msg::TomlInvalido(e.to_string()),
            ));
            return rel;
        }
    };

    let tabela = match valor.as_table() {
        Some(t) => t,
        None => {
            rel.diagnosticos
                .push(Diagnostico::erro(Local::Toml, Msg::NaoEhTabelaRaiz));
            return rel;
        }
    };

    // --- 1. schema_id (campo fixo, bootstrap) ---
    let bruto_id = match tabela.get(CAMPO_SCHEMA_ID) {
        Some(Value::String(s)) => s.clone(),
        Some(outro) => {
            rel.diagnosticos.push(Diagnostico::erro(
                campo(CAMPO_SCHEMA_ID),
                Msg::EsperadoTexto(outro.type_str().to_string()),
            ));
            return rel;
        }
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                campo(CAMPO_SCHEMA_ID),
                Msg::SchemaIdAusente,
            ));
            return rel;
        }
    };

    let (versao, idioma) = interpretar_schema_id(&bruto_id);
    rel.schema_id = Some(bruto_id.clone());
    rel.idioma = Some(idioma.clone());

    if versao != versao_esperada {
        rel.diagnosticos.push(Diagnostico::aviso(
            campo(CAMPO_SCHEMA_ID),
            Msg::VersaoDivergente {
                no_arquivo: versao,
                carregado: versao_esperada.to_string(),
            },
        ));
    }

    // --- 2. resolve o campo discriminador `tipo` ---
    let def_tipo = match schema.campos.get(CAMPO_TIPO) {
        Some(d) => d,
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                Local::Schema,
                Msg::SchemaSemCampoCanonico(CAMPO_TIPO.to_string()),
            ));
            return rel;
        }
    };

    let alias_tipo = match def_tipo.alias.get(&idioma) {
        Some(a) => a.clone(),
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                Local::Schema,
                Msg::SchemaSemAliasDoCampo {
                    campo: CAMPO_TIPO.to_string(),
                    idioma: idioma.clone(),
                },
            ));
            return rel;
        }
    };

    let bruto_tipo = match tabela.get(&alias_tipo) {
        Some(Value::String(s)) => s.clone(),
        Some(outro) => {
            rel.diagnosticos.push(Diagnostico::erro(
                campo(alias_tipo.clone()),
                Msg::EsperadoTexto(outro.type_str().to_string()),
            ));
            return rel;
        }
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                campo(alias_tipo.clone()),
                Msg::TipoAusente,
            ));
            return rel;
        }
    };

    let tipo_canonico = match resolver_enum(def_tipo, &bruto_tipo, &idioma) {
        Some(c) => c,
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                campo(alias_tipo.clone()),
                msg_enum_invalido(def_tipo, &bruto_tipo, &idioma),
            ));
            return rel;
        }
    };
    rel.tipo = Some(tipo_canonico.clone());

    // --- 3. conjunto de campos aplicáveis a este tipo ---
    let mut campos: BTreeMap<String, DefCampo> = schema.campos.clone();
    match tipo_canonico.as_str() {
        "caso_de_uso" => campos.extend(schema.campos_caso_de_uso.clone()),
        "ator" => campos.extend(schema.campos_ator.clone()),
        outro => rel.diagnosticos.push(Diagnostico::aviso(
            campo(alias_tipo.clone()),
            Msg::TipoSemCamposEspecificos(outro.to_string()),
        )),
    }

    // --- 4. mapas de alias ---
    let reverso = montar_reverso(&campos, &idioma, &mut rel.diagnosticos);
    let global = montar_global(&campos);

    // --- 5. percorre as chaves do arquivo ---
    let mut vistos: BTreeSet<String> = BTreeSet::new();
    for (chave, valor) in tabela {
        if chave.as_str() == CAMPO_SCHEMA_ID {
            continue;
        }
        match reverso.get(chave) {
            Some(canonico) => {
                vistos.insert(canonico.clone());
                rel.campos_reconhecidos += 1;
                if let Some(def) = campos.get(canonico) {
                    validar_valor(def, valor, &idioma, chave, &mut rel.diagnosticos);
                }
            }
            None => {
                let msg = match global.get(chave) {
                    Some((canonico, lang)) if lang != &idioma => Msg::AliasDeOutroIdioma {
                        canonico: canonico.clone(),
                        idioma_do_alias: lang.clone(),
                        idioma_declarado: idioma.clone(),
                        subcampo: false,
                    },
                    _ => Msg::CampoDesconhecido {
                        idioma: idioma.clone(),
                        subcampo: false,
                    },
                };
                rel.diagnosticos
                    .push(Diagnostico::erro(campo(chave.as_str()), msg));
            }
        }
    }

    // --- 6. campos obrigatórios ausentes ---
    for (canonico, def) in &campos {
        if def.obrigatorio && !vistos.contains(canonico) {
            let nome = def
                .alias
                .get(&idioma)
                .cloned()
                .unwrap_or_else(|| canonico.clone());
            rel.diagnosticos.push(Diagnostico::erro(
                campo(nome),
                Msg::ObrigatorioAusente {
                    canonico: canonico.clone(),
                    subcampo: false,
                },
            ));
        }
    }

    rel
}

// ============================================================
// Validação de valores
// ============================================================

fn validar_valor(
    def: &DefCampo,
    valor: &Value,
    idioma: &str,
    local: &str,
    diags: &mut Vec<Diagnostico>,
) {
    match def.tipo_dado.as_str() {
        "string" | "string_markdown" => {
            if valor.as_str().is_none() {
                diags.push(Diagnostico::erro(
                    campo(local),
                    Msg::EsperadoTexto(valor.type_str().to_string()),
                ));
            }
        }

        "array<string>" => match valor.as_array() {
            None => diags.push(Diagnostico::erro(
                campo(local),
                Msg::EsperadoListaTextos(valor.type_str().to_string()),
            )),
            Some(itens) => {
                if def.obrigatorio && itens.is_empty() {
                    diags.push(Diagnostico::erro(campo(local), Msg::ListaVaziaObrigatoria));
                }
                for (i, item) in itens.iter().enumerate() {
                    if item.as_str().is_none() {
                        diags.push(Diagnostico::erro(
                            campo(format!("{local}[{i}]")),
                            Msg::EsperadoTexto(item.type_str().to_string()),
                        ));
                    }
                }
            }
        },

        "enum" => match valor.as_str() {
            None => diags.push(Diagnostico::erro(
                campo(local),
                Msg::EsperadoTexto(valor.type_str().to_string()),
            )),
            Some(s) => {
                if resolver_enum(def, s, idioma).is_none() {
                    diags.push(Diagnostico::erro(
                        campo(local),
                        msg_enum_invalido(def, s, idioma),
                    ));
                }
            }
        },

        "array<tabela>" => match valor.as_array() {
            None => diags.push(Diagnostico::erro(
                campo(local),
                Msg::EsperadoListaTabelas(valor.type_str().to_string()),
            )),
            Some(itens) => {
                if def.obrigatorio && itens.is_empty() {
                    diags.push(Diagnostico::erro(campo(local), Msg::ListaVaziaObrigatoria));
                }
                for (i, item) in itens.iter().enumerate() {
                    let local_i = format!("{local}[{i}]");
                    match item.as_table() {
                        None => diags.push(Diagnostico::erro(
                            campo(local_i),
                            Msg::EsperadoTabela(item.type_str().to_string()),
                        )),
                        Some(t) => validar_subtabela(&def.subcampos, t, idioma, &local_i, diags),
                    }
                }
            }
        },

        outro => diags.push(Diagnostico::aviso(
            campo(local),
            Msg::TipoDadoDesconhecido(outro.to_string()),
        )),
    }
}

fn validar_subtabela(
    subcampos: &BTreeMap<String, DefCampo>,
    tabela: &toml::value::Table,
    idioma: &str,
    local: &str,
    diags: &mut Vec<Diagnostico>,
) {
    if subcampos.is_empty() {
        diags.push(Diagnostico::aviso(campo(local), Msg::SemSubcampos));
        return;
    }

    let reverso = montar_reverso(subcampos, idioma, diags);
    let global = montar_global(subcampos);

    let mut vistos: BTreeSet<String> = BTreeSet::new();
    for (chave, valor) in tabela {
        match reverso.get(chave) {
            Some(canonico) => {
                vistos.insert(canonico.clone());
                if let Some(def) = subcampos.get(canonico) {
                    validar_valor(def, valor, idioma, &format!("{local}.{chave}"), diags);
                }
            }
            None => {
                let msg = match global.get(chave) {
                    Some((canonico, lang)) if lang.as_str() != idioma => Msg::AliasDeOutroIdioma {
                        canonico: canonico.clone(),
                        idioma_do_alias: lang.clone(),
                        idioma_declarado: idioma.to_string(),
                        subcampo: true,
                    },
                    _ => Msg::CampoDesconhecido {
                        idioma: idioma.to_string(),
                        subcampo: true,
                    },
                };
                diags.push(Diagnostico::erro(campo(format!("{local}.{chave}")), msg));
            }
        }
    }

    for (canonico, def) in subcampos {
        if def.obrigatorio && !vistos.contains(canonico) {
            let nome = def
                .alias
                .get(idioma)
                .cloned()
                .unwrap_or_else(|| canonico.clone());
            diags.push(Diagnostico::erro(
                campo(format!("{local}.{nome}")),
                Msg::ObrigatorioAusente {
                    canonico: canonico.clone(),
                    subcampo: true,
                },
            ));
        }
    }
}

// ============================================================
// Auxiliares
// ============================================================

/// Divide "<versao>;<locale>" em (versao, idioma-base).
/// Sem ";" ou com locale vazio, o idioma é o padrão ("en"), de modo que
/// "x-v1" e "x-v1;" são equivalentes. De "pt_BR" usa-se apenas "pt".
fn interpretar_schema_id(bruto: &str) -> (String, String) {
    let mut partes = bruto.splitn(2, ';');
    let versao = partes.next().unwrap_or("").trim().to_string();
    let locale = partes.next().unwrap_or("").trim();

    let idioma = if locale.is_empty() {
        IDIOMA_PADRAO_CONTEUDO.to_string()
    } else {
        locale
            .split(['_', '-'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(IDIOMA_PADRAO_CONTEUDO)
            .to_lowercase()
    };

    (versao, idioma)
}

/// alias-no-idioma -> nome canônico, só para o idioma declarado.
fn montar_reverso(
    campos: &BTreeMap<String, DefCampo>,
    idioma: &str,
    diags: &mut Vec<Diagnostico>,
) -> BTreeMap<String, String> {
    let mut m: BTreeMap<String, String> = BTreeMap::new();
    for (canonico, def) in campos {
        match def.alias.get(idioma) {
            Some(a) => {
                if let Some(anterior) = m.insert(a.clone(), canonico.clone()) {
                    diags.push(Diagnostico::aviso(
                        Local::Schema,
                        Msg::AliasAmbiguo {
                            alias: a.clone(),
                            idioma: idioma.to_string(),
                            anterior,
                            atual: canonico.clone(),
                        },
                    ));
                }
            }
            None => diags.push(Diagnostico::aviso(
                Local::Schema,
                Msg::SemAliasNoIdioma {
                    canonico: canonico.clone(),
                    idioma: idioma.to_string(),
                },
            )),
        }
    }
    m
}

/// alias-em-qualquer-idioma -> (canônico, idioma). Usado só para gerar
/// mensagens úteis quando o autor mistura idiomas.
fn montar_global(campos: &BTreeMap<String, DefCampo>) -> BTreeMap<String, (String, String)> {
    let mut m = BTreeMap::new();
    for (canonico, def) in campos {
        for (lang, alias) in &def.alias {
            m.entry(alias.clone())
                .or_insert_with(|| (canonico.clone(), lang.clone()));
        }
    }
    m
}

/// Converte o valor escrito no arquivo para o valor canônico do enum.
fn resolver_enum(def: &DefCampo, bruto: &str, idioma: &str) -> Option<String> {
    def.valores
        .iter()
        .find(|(_, v)| v.alias.get(idioma).map(|a| a == bruto).unwrap_or(false))
        .map(|(canonico, _)| canonico.clone())
}

fn msg_enum_invalido(def: &DefCampo, bruto: &str, idioma: &str) -> Msg {
    let permitidos: Vec<String> = def
        .valores
        .values()
        .filter_map(|v| v.alias.get(idioma).cloned())
        .collect();

    // O valor existe, mas em outro idioma? Mensagem mais útil que "inválido".
    let em_outro_idioma = def.valores.iter().find_map(|(canonico, v)| {
        v.alias
            .iter()
            .find(|(lang, alias)| lang.as_str() != idioma && alias.as_str() == bruto)
            .map(|(lang, _)| (canonico.clone(), lang.clone()))
    });

    Msg::EnumInvalido {
        valor: bruto.to_string(),
        idioma: idioma.to_string(),
        permitidos,
        em_outro_idioma,
    }
}

// ============================================================
// Testes unitários
// ============================================================
// Ficam aqui dentro (e não em tests/) porque `reqmeshval` é um crate
// binário: testes de integração em tests/ não conseguem importar itens
// de um bin, só de uma lib. Como módulo interno, estes testes enxergam
// funções e campos privados. `#[cfg(test)]` faz o bloco existir apenas
// sob `cargo test` — não entra no binário de produção.

#[cfg(test)]
mod testes {
    use super::*;

    fn fixture(nome: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(nome)
    }

    fn schema_de_teste() -> Schema {
        let bruto = fs::read_to_string(fixture("elemento-requisito-v1.toml"))
            .expect("fixture do schema não encontrada");
        toml::from_str(&bruto).expect("fixture do schema não parseia")
    }

    /// Uma instância de cada variante de `Msg`, para garantir que toda
    /// mensagem que o código sabe emitir existe no catálogo.
    fn uma_de_cada_mensagem() -> Vec<Msg> {
        let s = || "x".to_string();
        vec![
            Msg::ArquivoIlegivel(s()),
            Msg::TomlInvalido(s()),
            Msg::NaoEhTabelaRaiz,
            Msg::SchemaIdAusente,
            Msg::VersaoDivergente {
                no_arquivo: s(),
                carregado: s(),
            },
            Msg::SchemaSemCampoCanonico(s()),
            Msg::SchemaSemAliasDoCampo {
                campo: s(),
                idioma: s(),
            },
            Msg::TipoAusente,
            Msg::TipoSemCamposEspecificos(s()),
            Msg::EsperadoTexto(s()),
            Msg::EsperadoListaTextos(s()),
            Msg::EsperadoListaTabelas(s()),
            Msg::EsperadoTabela(s()),
            Msg::ListaVaziaObrigatoria,
            Msg::EnumInvalido {
                valor: s(),
                idioma: s(),
                permitidos: vec![s()],
                em_outro_idioma: None,
            },
            Msg::EnumInvalido {
                valor: s(),
                idioma: s(),
                permitidos: vec![s()],
                em_outro_idioma: Some((s(), s())),
            },
            Msg::AliasDeOutroIdioma {
                canonico: s(),
                idioma_do_alias: s(),
                idioma_declarado: s(),
                subcampo: false,
            },
            Msg::AliasDeOutroIdioma {
                canonico: s(),
                idioma_do_alias: s(),
                idioma_declarado: s(),
                subcampo: true,
            },
            Msg::CampoDesconhecido {
                idioma: s(),
                subcampo: false,
            },
            Msg::CampoDesconhecido {
                idioma: s(),
                subcampo: true,
            },
            Msg::ObrigatorioAusente {
                canonico: s(),
                subcampo: false,
            },
            Msg::ObrigatorioAusente {
                canonico: s(),
                subcampo: true,
            },
            Msg::TipoDadoDesconhecido(s()),
            Msg::SemSubcampos,
            Msg::AliasAmbiguo {
                alias: s(),
                idioma: s(),
                anterior: s(),
                atual: s(),
            },
            Msg::SemAliasNoIdioma {
                canonico: s(),
                idioma: s(),
            },
        ]
    }

    // --- catálogos -------------------------------------------------

    #[test]
    fn catalogos_embutidos_parseiam() {
        let idiomas = Idiomas::carregar(None);
        for (codigo, _) in EMBUTIDOS {
            assert!(
                idiomas.tem(codigo),
                "catálogo embutido '{codigo}' não carregou"
            );
        }
    }

    /// O fallback silencioso para `en` esconderia uma chave faltando em
    /// pt, então a paridade é comparada direto entre os catálogos, sem
    /// passar pela resolução normal.
    #[test]
    fn todos_os_catalogos_tem_as_mesmas_chaves() {
        let idiomas = Idiomas::carregar(None);
        let base = idiomas
            .catalogos
            .get(FALLBACK)
            .expect("catálogo de fallback ausente");

        for (codigo, cat) in &idiomas.catalogos {
            let ui_base: BTreeSet<_> = base.ui.keys().collect();
            let ui_cat: BTreeSet<_> = cat.ui.keys().collect();
            assert_eq!(ui_base, ui_cat, "seção [ui] diverge em '{codigo}'");

            let msg_base: BTreeSet<_> = base.msg.keys().collect();
            let msg_cat: BTreeSet<_> = cat.msg.keys().collect();
            assert_eq!(msg_base, msg_cat, "seção [msg] diverge em '{codigo}'");

            let pl_base: BTreeSet<_> = base.plural.keys().collect();
            let pl_cat: BTreeSet<_> = cat.plural.keys().collect();
            assert_eq!(pl_base, pl_cat, "seção [plural] diverge em '{codigo}'");
        }
    }

    /// Pega os dois defeitos que só apareceriam em runtime: chave que o
    /// código usa e não existe em catálogo nenhum (sai como ⟨chave⟩), e
    /// placeholder que a tradução esqueceu de consumir (fica {literal}).
    #[test]
    fn toda_mensagem_renderiza_completa_em_todo_idioma() {
        let idiomas = Idiomas::carregar(None);
        for codigo in idiomas.disponiveis() {
            for m in uma_de_cada_mensagem() {
                let texto = m.render(&idiomas, &codigo);
                assert!(
                    !texto.contains('⟨'),
                    "chave ausente em '{codigo}': {texto}"
                );
                assert!(
                    !texto.contains('{'),
                    "placeholder não substituído em '{codigo}': {texto}"
                );
            }
        }
    }

    #[test]
    fn plurais_selecionam_a_forma_certa() {
        let idiomas = Idiomas::carregar(None);
        assert_eq!(idiomas.plural("pt", "erros", 1), "1 erro");
        assert_eq!(idiomas.plural("pt", "erros", 0), "0 erros");
        assert_eq!(idiomas.plural("pt", "erros", 3), "3 erros");
        assert_eq!(idiomas.plural("en", "erros", 1), "1 error");
        assert_eq!(idiomas.plural("en", "erros", 2), "2 errors");
    }

    #[test]
    fn idioma_sem_catalogo_cai_no_fallback() {
        let idiomas = Idiomas::carregar(None);
        assert_eq!(idiomas.efetivo("pt"), "pt");
        assert_eq!(idiomas.efetivo("tlh"), FALLBACK);
    }

    // --- schema_id -------------------------------------------------

    #[test]
    fn schema_id_sem_locale_usa_o_idioma_padrao() {
        // "x;" e "x" precisam ser equivalentes.
        assert_eq!(
            interpretar_schema_id("elemento-requisito-v1"),
            ("elemento-requisito-v1".to_string(), "en".to_string())
        );
        assert_eq!(
            interpretar_schema_id("elemento-requisito-v1;"),
            ("elemento-requisito-v1".to_string(), "en".to_string())
        );
    }

    #[test]
    fn schema_id_usa_apenas_o_idioma_base_do_locale() {
        for entrada in [
            "elemento-requisito-v1;pt_BR",
            "elemento-requisito-v1;pt-br",
            "elemento-requisito-v1; PT_br ",
            "elemento-requisito-v1;pt",
        ] {
            let (_, idioma) = interpretar_schema_id(entrada);
            assert_eq!(idioma, "pt", "falhou para {entrada}");
        }
    }

    // --- validação -------------------------------------------------

    #[test]
    fn exemplos_validos_passam() {
        let schema = schema_de_teste();
        for nome in ["exemplo-pt.toml", "exemplo-en.toml"] {
            let rel = validar_arquivo(&schema, "elemento-requisito-v1", &fixture(nome));
            let chaves: Vec<&str> = rel
                .diagnosticos
                .iter()
                .map(|d| d.msg.chave_e_args().0)
                .collect();
            assert!(rel.valido(), "{nome} deveria passar; erros: {chaves:?}");
        }
    }

    /// Os dois exemplos descrevem o mesmo caso de uso em idiomas
    /// diferentes, então precisam normalizar para o mesmo tipo canônico.
    #[test]
    fn exemplos_pt_e_en_normalizam_para_o_mesmo_tipo() {
        let schema = schema_de_teste();
        let pt = validar_arquivo(&schema, "elemento-requisito-v1", &fixture("exemplo-pt.toml"));
        let en = validar_arquivo(&schema, "elemento-requisito-v1", &fixture("exemplo-en.toml"));
        assert_eq!(pt.tipo, Some("caso_de_uso".to_string()));
        assert_eq!(pt.tipo, en.tipo);
        assert_eq!(pt.campos_reconhecidos, en.campos_reconhecidos);
    }

    #[test]
    fn fixture_invalida_reporta_cada_defeito_embutido() {
        let schema = schema_de_teste();
        let rel = validar_arquivo(
            &schema,
            "elemento-requisito-v1",
            &fixture("exemplo-invalido.toml"),
        );
        assert!(!rel.valido());

        let chaves: Vec<&str> = rel
            .diagnosticos
            .iter()
            .map(|d| d.msg.chave_e_args().0)
            .collect();

        for esperada in [
            "alias-de-outro-idioma",          // "name" num arquivo pt
            "enum-invalido-outro-idioma",     // prioridade = "high"
            "obrigatorio-ausente",            // fluxo_principal
            "esperado-texto",                 // gatilho = 42 / item numérico
            "campo-desconhecido",             // "cor"
            "subcampo-obrigatorio-ausente",   // relacoes[0].alvo
        ] {
            assert!(
                chaves.contains(&esperada),
                "esperava '{esperada}' entre os diagnósticos; obtive {chaves:?}"
            );
        }
    }

    #[test]
    fn arquivo_inexistente_nao_entra_em_panico() {
        let schema = schema_de_teste();
        let rel = validar_arquivo(&schema, "elemento-requisito-v1", &fixture("nao-existe.toml"));
        assert!(!rel.valido());
        assert_eq!(
            rel.diagnosticos[0].msg.chave_e_args().0,
            "arquivo-ilegivel"
        );
    }
}
