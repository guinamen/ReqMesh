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
//!
//! O binário (`src/main.rs`) é só uma casca sobre [`executar`]; toda a
//! lógica — inclusive o parsing da linha de comando — vive aqui, para
//! ficar acessível a testes de integração em `tests/`.
//!
//! Pipeline de [`executar`]:
//!
//! ```text
//! dir_catalogos → Idiomas::carregar → parse_cli → carregar_schema
//!   → executar_validacao → montar_log → (stdout, arquivo de log, ExitCode)
//! ```
//!
//! As quatro etapas centrais são funções que não imprimem nada: erros
//! saem como dados (`ErroCli`, `ErroSchema`, `Diagnostico`) e só viram
//! texto na borda, dentro de `executar`.

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
pub const FALLBACK: &str = "en";

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
pub struct Idiomas {
    catalogos: BTreeMap<String, Catalogo>,
}

impl Idiomas {
    /// Carrega os catálogos embutidos e, se informado, os do diretório
    /// (que sobrepõem os embutidos de mesmo código).
    pub fn carregar(dir: Option<&Path>) -> Idiomas {
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

    pub fn tem(&self, codigo: &str) -> bool {
        self.catalogos.contains_key(codigo)
    }

    pub fn disponiveis(&self) -> Vec<String> {
        self.catalogos.keys().cloned().collect()
    }

    /// Resolve o idioma efetivo: o pedido, se existir catálogo; senão o
    /// fallback.
    pub fn efetivo(&self, codigo: &str) -> String {
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

    pub fn ui(&self, codigo: &str, chave: &str, args: &[(&str, String)]) -> String {
        interpolar(&self.texto(codigo, Secao::Ui, chave), args)
    }

    /// Atalho para rótulos sem argumentos.
    pub fn rot(&self, codigo: &str, chave: &str) -> String {
        self.texto(codigo, Secao::Ui, chave)
    }

    pub fn msg(&self, codigo: &str, chave: &str, args: &[(&str, String)]) -> String {
        interpolar(&self.texto(codigo, Secao::Msg, chave), args)
    }

    /// Seleção de forma plural. Regra simples: 1 -> `one`, resto -> `other`.
    /// Cobre pt e en; idiomas com regras mais ricas (russo, polonês, árabe)
    /// precisariam de categorias adicionais no catálogo.
    pub fn plural(&self, codigo: &str, chave: &str, n: usize) -> String {
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

pub fn idioma_do_ambiente() -> Option<String> {
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
pub struct Schema {
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
pub enum Nivel {
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
pub enum Local {
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
pub enum Msg {
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
    pub fn chave_e_args(&self) -> (&'static str, Vec<(&'static str, String)>) {
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
            Msg::ObrigatorioAusente { canonico, subcampo } => {
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

    pub fn render(&self, idiomas: &Idiomas, codigo: &str) -> String {
        let (chave, args) = self.chave_e_args();
        idiomas.msg(codigo, chave, &args)
    }
}

#[derive(Debug)]
pub struct Diagnostico {
    pub nivel: Nivel,
    pub local: Local,
    pub msg: Msg,
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
pub struct Relatorio {
    pub schema_id: Option<String>,
    pub idioma: Option<String>,
    pub tipo: Option<String>,
    pub campos_reconhecidos: usize,
    pub diagnosticos: Vec<Diagnostico>,
}

impl Relatorio {
    pub fn erros(&self) -> usize {
        self.diagnosticos
            .iter()
            .filter(|d| d.nivel == Nivel::Erro)
            .count()
    }

    pub fn avisos(&self) -> usize {
        self.diagnosticos
            .iter()
            .filter(|d| d.nivel == Nivel::Aviso)
            .count()
    }

    pub fn valido(&self) -> bool {
        self.erros() == 0
    }

    /// Chaves de catálogo dos diagnósticos, na ordem em que saíram.
    /// Comparar chaves em vez de texto renderizado mantém os testes
    /// independentes da redação dos catálogos.
    pub fn chaves(&self) -> Vec<&'static str> {
        self.diagnosticos
            .iter()
            .map(|d| d.msg.chave_e_args().0)
            .collect()
    }
}

// ============================================================
// Linha de comando (dados, não impressão)
// ============================================================

/// Configuração de uma execução de validação, já resolvida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub caminho_schema: PathBuf,
    pub arquivos: Vec<PathBuf>,
    pub caminho_log: Option<PathBuf>,
    /// `--idioma auto`: cada arquivo é reportado no idioma que declara.
    pub auto: bool,
    /// Idioma do cabeçalho, do resumo e das mensagens fora de arquivo.
    pub l_geral: String,
    /// Idioma do ambiente já resolvido; usado como fallback no modo auto
    /// quando o arquivo não chega a declarar o seu.
    pub padrao: String,
}

/// O que a linha de comando pediu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acao {
    /// `--help` / `--idiomas`: imprimir e sair com sucesso.
    Imprimir(String),
    Validar(Config),
}

/// Erros de uso da CLI. Como os `Msg`, guardam dados e só viram texto na
/// renderização — é o que permite testar o parsing sem capturar stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErroCli {
    FaltamArgs,
    LogSemCaminho,
    IdiomaSemValor,
    IdiomaInvalido {
        idioma: String,
        disponiveis: Vec<String>,
    },
}

impl ErroCli {
    pub fn chave_e_args(&self) -> (&'static str, Vec<(&'static str, String)>) {
        match self {
            ErroCli::FaltamArgs => ("cli-faltam-args", vec![]),
            ErroCli::LogSemCaminho => ("cli-log-sem-caminho", vec![]),
            ErroCli::IdiomaSemValor => ("cli-idioma-sem-valor", vec![]),
            ErroCli::IdiomaInvalido {
                idioma,
                disponiveis,
            } => (
                "cli-idioma-invalido",
                vec![
                    ("idioma", idioma.clone()),
                    ("disponiveis", disponiveis.join(", ")),
                ],
            ),
        }
    }

    /// Erro de forma de invocação vem acompanhado do texto de uso; erro
    /// de valor não, porque a própria mensagem já diz o que aceitar.
    fn mostra_uso(&self) -> bool {
        matches!(self, ErroCli::FaltamArgs | ErroCli::LogSemCaminho)
    }

    pub fn render(&self, idiomas: &Idiomas, codigo: &str) -> String {
        let (chave, args) = self.chave_e_args();
        let texto = idiomas.ui(codigo, &chave, &args);
        if self.mostra_uso() {
            format!("{texto}\n\n{}", idiomas.rot(codigo, "uso"))
        } else {
            texto
        }
    }
}

/// Falhas ao carregar o schema — fatais, mas ainda assim dados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErroSchema {
    Ilegivel { caminho: PathBuf, erro: String },
    Invalido { caminho: PathBuf, erro: String },
}

impl ErroSchema {
    pub fn chave_e_args(&self) -> (&'static str, Vec<(&'static str, String)>) {
        let (chave, caminho, erro) = match self {
            ErroSchema::Ilegivel { caminho, erro } => ("schema-ilegivel", caminho, erro),
            ErroSchema::Invalido { caminho, erro } => ("schema-invalido", caminho, erro),
        };
        (
            chave,
            vec![
                ("caminho", caminho.display().to_string()),
                ("erro", erro.clone()),
            ],
        )
    }

    pub fn render(&self, idiomas: &Idiomas, codigo: &str) -> String {
        let (chave, args) = self.chave_e_args();
        idiomas.ui(codigo, chave, &args)
    }
}

/// Varredura prévia por `--catalogos`: o diretório precisa ser conhecido
/// antes de existir qualquer catálogo, portanto antes de `parse_cli`,
/// que já depende de `Idiomas` para validar `--idioma`.
pub fn dir_catalogos(args: &[String]) -> Option<PathBuf> {
    let mut dir = None;
    for (i, a) in args.iter().enumerate() {
        if (a == "-c" || a == "--catalogos") && i + 1 < args.len() {
            dir = Some(PathBuf::from(&args[i + 1]));
        }
    }
    dir
}

/// Interpreta os argumentos (já sem o `argv[0]`). Não imprime nem sai:
/// devolve o que fazer, ou o erro como dado.
pub fn parse_cli(args: &[String], idiomas: &Idiomas, padrao: &str) -> Result<Acao, ErroCli> {
    let mut posicionais: Vec<String> = Vec::new();
    let mut caminho_log: Option<PathBuf> = None;
    // None = seguir o padrão; Some(None) = auto; Some(Some(cod)) = fixo.
    let mut idioma_pedido: Option<Option<String>> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => return Ok(Acao::Imprimir(idiomas.rot(padrao, "uso"))),
            "--idiomas" => return Ok(Acao::Imprimir(idiomas.disponiveis().join("\n"))),
            "-c" | "--catalogos" => {
                i += 1; // já tratado por dir_catalogos
            }
            "-l" | "--log" => {
                i += 1;
                match args.get(i) {
                    Some(v) => caminho_log = Some(PathBuf::from(v)),
                    None => return Err(ErroCli::LogSemCaminho),
                }
            }
            "-i" | "--idioma" | "--lang" => {
                i += 1;
                match args.get(i) {
                    Some(v) if v.eq_ignore_ascii_case("auto") => idioma_pedido = Some(None),
                    Some(v) => {
                        let cod = v.to_lowercase();
                        if !idiomas.tem(&cod) {
                            return Err(ErroCli::IdiomaInvalido {
                                idioma: cod,
                                disponiveis: idiomas.disponiveis(),
                            });
                        }
                        idioma_pedido = Some(Some(cod));
                    }
                    None => return Err(ErroCli::IdiomaSemValor),
                }
            }
            outro => posicionais.push(outro.to_string()),
        }
        i += 1;
    }

    if posicionais.len() < 2 {
        return Err(ErroCli::FaltamArgs);
    }

    Ok(Acao::Validar(Config {
        caminho_schema: PathBuf::from(&posicionais[0]),
        arquivos: posicionais[1..].iter().map(PathBuf::from).collect(),
        caminho_log,
        auto: matches!(idioma_pedido, Some(None)),
        l_geral: match &idioma_pedido {
            Some(Some(cod)) => cod.clone(),
            _ => padrao.to_string(),
        },
        padrao: padrao.to_string(),
    }))
}

// ============================================================
// Etapas da execução
// ============================================================

/// A versão esperada é o nome do arquivo de schema sem extensão.
fn versao_do_caminho(caminho: &Path) -> String {
    caminho
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Lê e parseia o schema, devolvendo-o junto da versão esperada.
pub fn carregar_schema(caminho: &Path) -> Result<(Schema, String), ErroSchema> {
    let bruto = fs::read_to_string(caminho).map_err(|e| ErroSchema::Ilegivel {
        caminho: caminho.to_path_buf(),
        erro: e.to_string(),
    })?;

    let schema: Schema = toml::from_str(&bruto).map_err(|e| ErroSchema::Invalido {
        caminho: caminho.to_path_buf(),
        erro: e.to_string(),
    })?;

    Ok((schema, versao_do_caminho(caminho)))
}

/// Valida cada arquivo. Não formata nada: a decisão de idioma por arquivo
/// é do `montar_log`, que lê o idioma declarado dentro do `Relatorio`.
pub fn executar_validacao(
    schema: &Schema,
    versao: &str,
    arquivos: &[PathBuf],
) -> Vec<(PathBuf, Relatorio)> {
    arquivos
        .iter()
        .map(|a| (a.clone(), validar_arquivo(schema, versao, a)))
        .collect()
}

/// Renderiza cabeçalho, blocos por arquivo e resumo. Função pura: mesma
/// entrada, mesmo texto — exceto pelo carimbo de data/hora do cabeçalho.
pub fn montar_log(cfg: &Config, resultados: &[(PathBuf, Relatorio)], idiomas: &Idiomas) -> String {
    let l = cfg.l_geral.as_str();
    let versao = versao_do_caminho(&cfg.caminho_schema);
    let descricao_idioma = if cfg.auto {
        "auto".to_string()
    } else {
        cfg.l_geral.clone()
    };

    let mut log = String::new();
    log.push_str(&format!("{}\n", idiomas.rot(l, "titulo")));
    log.push_str(&format!(
        "{:<9}: {}\n",
        idiomas.rot(l, "data"),
        chrono::Local::now().to_rfc3339()
    ));
    log.push_str(&format!(
        "{:<9}: {}\n",
        idiomas.rot(l, "schema"),
        cfg.caminho_schema.display()
    ));
    log.push_str(&format!("{:<9}: {versao}\n", idiomas.rot(l, "versao")));
    log.push_str(&format!(
        "{:<9}: {}\n",
        idiomas.rot(l, "arquivos"),
        resultados.len()
    ));
    log.push_str(&format!(
        "{:<9}: {descricao_idioma}\n\n",
        idiomas.rot(l, "idioma")
    ));

    let total = resultados.len();
    let mut validos = 0usize;
    let mut total_erros = 0usize;
    let mut total_avisos = 0usize;

    for (idx, (arquivo, rel)) in resultados.iter().enumerate() {
        if rel.valido() {
            validos += 1;
        }
        total_erros += rel.erros();
        total_avisos += rel.avisos();

        // No modo auto, cada arquivo é reportado no idioma que ele declara;
        // idioma sem catálogo cai no fallback.
        let l_arquivo = if cfg.auto {
            idiomas.efetivo(rel.idioma.as_deref().unwrap_or(&cfg.padrao))
        } else {
            cfg.l_geral.clone()
        };

        log.push_str(&formatar_arquivo(
            idx + 1,
            total,
            arquivo,
            rel,
            idiomas,
            &l_arquivo,
        ));
    }

    log.push_str(&format!("{}\n", idiomas.rot(l, "resumo")));
    for (chave, valor) in [
        ("arquivos", total),
        ("validos", validos),
        ("falhas", total - validos),
        ("erros", total_erros),
        ("avisos", total_avisos),
    ] {
        log.push_str(&format!("{:<9}: {valor}\n", idiomas.rot(l, chave)));
    }

    log
}

/// Cola: as quatro etapas em sequência, mais a única parte que fala com o
/// mundo (stdout/stderr, arquivo de log, código de saída).
pub fn executar<I: IntoIterator<Item = String>>(args: I) -> ExitCode {
    let args: Vec<String> = args.into_iter().collect();

    let idiomas = Idiomas::carregar(dir_catalogos(&args).as_deref());
    let padrao = idiomas.efetivo(&idioma_do_ambiente().unwrap_or_default());

    let cfg = match parse_cli(&args, &idiomas, &padrao) {
        Ok(Acao::Imprimir(texto)) => {
            println!("{texto}");
            return ExitCode::SUCCESS;
        }
        Ok(Acao::Validar(cfg)) => cfg,
        Err(e) => {
            eprintln!("{}", e.render(&idiomas, &padrao));
            return ExitCode::from(2);
        }
    };

    let (schema, versao) = match carregar_schema(&cfg.caminho_schema) {
        Ok(par) => par,
        Err(e) => {
            eprintln!("{}", e.render(&idiomas, &cfg.l_geral));
            return ExitCode::from(2);
        }
    };

    let resultados = executar_validacao(&schema, &versao, &cfg.arquivos);
    let log = montar_log(&cfg, &resultados, &idiomas);

    print!("{log}");

    if let Some(caminho) = &cfg.caminho_log {
        if let Err(e) = fs::write(caminho, &log) {
            eprintln!(
                "{}",
                idiomas.ui(
                    &cfg.l_geral,
                    "log-nao-gravado",
                    &[
                        ("caminho", caminho.display().to_string()),
                        ("erro", e.to_string()),
                    ],
                )
            );
        }
    }

    let houve_erro = resultados.iter().any(|(_, r)| !r.valido());
    if houve_erro {
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

pub fn validar_arquivo(schema: &Schema, versao_esperada: &str, caminho: &Path) -> Relatorio {
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
    let bruto_id = match ler_campo_texto(tabela, CAMPO_SCHEMA_ID, Msg::SchemaIdAusente) {
        Ok(s) => s,
        Err(d) => {
            rel.diagnosticos.push(d);
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

    let bruto_tipo = match ler_campo_texto(tabela, &alias_tipo, Msg::TipoAusente) {
        Ok(s) => s,
        Err(d) => {
            rel.diagnosticos.push(d);
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

    // --- 4. percorre as chaves do arquivo e cobra os obrigatórios ---
    let (reconhecidos, _vistos) = validar_campos(
        &campos,
        tabela,
        &idioma,
        "",
        false,
        &mut rel.diagnosticos,
    );
    rel.campos_reconhecidos = reconhecidos;

    rel
}

// ============================================================
// Validação de valores
// ============================================================

/// Núcleo compartilhado entre a raiz do arquivo e as subtabelas: resolver
/// alias, acusar campo desconhecido ou de outro idioma, validar cada valor
/// e cobrar os obrigatórios ausentes.
///
/// `prefixo_local` é "" na raiz e o caminho do item ("relacoes[0]") dentro
/// de subtabela; `subcampo` escolhe a família de mensagens e, junto com o
/// prefixo, é a única diferença entre os dois usos.
///
/// Devolve (campos reconhecidos, nomes canônicos vistos).
fn validar_campos(
    campos: &BTreeMap<String, DefCampo>,
    tabela: &toml::value::Table,
    idioma: &str,
    prefixo_local: &str,
    subcampo: bool,
    diags: &mut Vec<Diagnostico>,
) -> (usize, BTreeSet<String>) {
    let reverso = montar_reverso(campos, idioma, diags);
    let global = montar_global(campos);

    let mut reconhecidos = 0usize;
    let mut vistos: BTreeSet<String> = BTreeSet::new();

    for (chave, valor) in tabela {
        // `schema_id` é o campo fixo de bootstrap: só existe na raiz, e lá
        // já foi lido antes desta função.
        if !subcampo && chave.as_str() == CAMPO_SCHEMA_ID {
            continue;
        }

        let local = juntar(prefixo_local, chave);

        match reverso.get(chave) {
            Some(canonico) => {
                vistos.insert(canonico.clone());
                reconhecidos += 1;
                if let Some(def) = campos.get(canonico) {
                    validar_valor(def, valor, idioma, &local, diags);
                }
            }
            None => {
                let msg = match global.get(chave) {
                    Some((canonico, lang)) if lang.as_str() != idioma => Msg::AliasDeOutroIdioma {
                        canonico: canonico.clone(),
                        idioma_do_alias: lang.clone(),
                        idioma_declarado: idioma.to_string(),
                        subcampo,
                    },
                    _ => Msg::CampoDesconhecido {
                        idioma: idioma.to_string(),
                        subcampo,
                    },
                };
                diags.push(Diagnostico::erro(campo(local), msg));
            }
        }
    }

    for (canonico, def) in campos {
        if def.obrigatorio && !vistos.contains(canonico) {
            let nome = def
                .alias
                .get(idioma)
                .cloned()
                .unwrap_or_else(|| canonico.clone());
            diags.push(Diagnostico::erro(
                campo(juntar(prefixo_local, &nome)),
                Msg::ObrigatorioAusente {
                    canonico: canonico.clone(),
                    subcampo,
                },
            ));
        }
    }

    (reconhecidos, vistos)
}

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
    // Schema que declara `array<tabela>` sem subcampos não tem como
    // validar nada dentro dos itens; avisa e para.
    if subcampos.is_empty() {
        diags.push(Diagnostico::aviso(campo(local), Msg::SemSubcampos));
        return;
    }

    validar_campos(subcampos, tabela, idioma, local, true, diags);
}

// ============================================================
// Auxiliares
// ============================================================

/// Compõe o caminho de um campo. Na raiz o prefixo é vazio e o caminho é
/// a própria chave.
fn juntar(prefixo: &str, chave: &str) -> String {
    if prefixo.is_empty() {
        chave.to_string()
    } else {
        format!("{prefixo}.{chave}")
    }
}

/// Lê um campo de texto obrigatório, distinguindo os três casos: presente
/// e string, presente com o tipo errado, ausente. O diagnóstico de ausência
/// é parâmetro porque só o chamador sabe o que significa faltar aquele campo.
fn ler_campo_texto(
    tabela: &toml::value::Table,
    chave: &str,
    ausente: Msg,
) -> Result<String, Diagnostico> {
    match tabela.get(chave) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(outro) => Err(Diagnostico::erro(
            campo(chave),
            Msg::EsperadoTexto(outro.type_str().to_string()),
        )),
        None => Err(Diagnostico::erro(campo(chave), ausente)),
    }
}

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
// Ficam dentro da lib (e não em tests/) porque enxergam itens privados:
// `interpretar_schema_id`, `ler_campo_texto`, `validar_campos`, a estrutura
// interna de `Catalogo`. O que se apoia só na API pública vive em
// tests/integracao.rs. `#[cfg(test)]` mantém o bloco fora do binário.

#[cfg(test)]
mod testes {
    use super::*;

    fn fixture(nome: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(nome)
    }

    fn schema_de_teste() -> Schema {
        carregar_schema(&fixture("elemento-requisito-v1.toml"))
            .expect("fixture do schema não carrega")
            .0
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
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

    fn um_de_cada_erro_cli() -> Vec<ErroCli> {
        vec![
            ErroCli::FaltamArgs,
            ErroCli::LogSemCaminho,
            ErroCli::IdiomaSemValor,
            ErroCli::IdiomaInvalido {
                idioma: "x".to_string(),
                disponiveis: vec!["en".to_string()],
            },
        ]
    }

    fn um_de_cada_erro_schema() -> Vec<ErroSchema> {
        vec![
            ErroSchema::Ilegivel {
                caminho: PathBuf::from("x"),
                erro: "x".to_string(),
            },
            ErroSchema::Invalido {
                caminho: PathBuf::from("x"),
                erro: "x".to_string(),
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
                assert!(!texto.contains('⟨'), "chave ausente em '{codigo}': {texto}");
                assert!(
                    !texto.contains('{'),
                    "placeholder não substituído em '{codigo}': {texto}"
                );
            }
        }
    }

    /// Mesma garantia para as mensagens de CLI e de schema, que também
    /// passam pelos catálogos (seção [ui]).
    #[test]
    fn todo_erro_de_cli_e_de_schema_renderiza_completo() {
        let idiomas = Idiomas::carregar(None);
        for codigo in idiomas.disponiveis() {
            let mut textos: Vec<String> = um_de_cada_erro_cli()
                .iter()
                .map(|e| e.render(&idiomas, &codigo))
                .collect();
            textos.extend(
                um_de_cada_erro_schema()
                    .iter()
                    .map(|e| e.render(&idiomas, &codigo)),
            );
            for texto in textos {
                assert!(!texto.contains('⟨'), "chave ausente em '{codigo}': {texto}");
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

    // --- CLI -------------------------------------------------------

    fn cfg_de(argumentos: &[&str]) -> Config {
        let idiomas = Idiomas::carregar(None);
        match parse_cli(&args(argumentos), &idiomas, "en") {
            Ok(Acao::Validar(cfg)) => cfg,
            outro => panic!("esperava Validar, obtive {outro:?}"),
        }
    }

    fn erro_de(argumentos: &[&str]) -> ErroCli {
        let idiomas = Idiomas::carregar(None);
        match parse_cli(&args(argumentos), &idiomas, "en") {
            Err(e) => e,
            outro => panic!("esperava erro, obtive {outro:?}"),
        }
    }

    #[test]
    fn cli_separa_schema_dos_arquivos() {
        let cfg = cfg_de(&["s.toml", "a.toml", "b.toml"]);
        assert_eq!(cfg.caminho_schema, PathBuf::from("s.toml"));
        assert_eq!(
            cfg.arquivos,
            vec![PathBuf::from("a.toml"), PathBuf::from("b.toml")]
        );
        assert_eq!(cfg.caminho_log, None);
    }

    #[test]
    fn cli_sem_idioma_usa_o_padrao() {
        let cfg = cfg_de(&["s.toml", "a.toml"]);
        assert!(!cfg.auto);
        assert_eq!(cfg.l_geral, "en");
    }

    #[test]
    fn cli_idioma_fixo_normaliza_para_minusculas() {
        for forma in [
            vec!["-i", "PT", "s.toml", "a.toml"],
            vec!["--idioma", "pt", "s.toml", "a.toml"],
            vec!["--lang", "pt", "s.toml", "a.toml"],
        ] {
            let cfg = cfg_de(&forma);
            assert!(!cfg.auto, "{forma:?}");
            assert_eq!(cfg.l_geral, "pt", "{forma:?}");
        }
    }

    #[test]
    fn cli_idioma_auto_nao_fixa_l_geral() {
        // No modo auto o cabeçalho segue o padrão; quem varia é o bloco
        // de cada arquivo.
        let cfg = cfg_de(&["-i", "AUTO", "s.toml", "a.toml"]);
        assert!(cfg.auto);
        assert_eq!(cfg.l_geral, "en");
    }

    #[test]
    fn cli_ultima_ocorrencia_de_idioma_vence() {
        let cfg = cfg_de(&["-i", "pt", "-i", "auto", "s.toml", "a.toml"]);
        assert!(cfg.auto);
    }

    #[test]
    fn cli_log_guarda_o_caminho() {
        let cfg = cfg_de(&["-l", "saida.log", "s.toml", "a.toml"]);
        assert_eq!(cfg.caminho_log, Some(PathBuf::from("saida.log")));
    }

    /// O valor de `-c` não pode virar posicional: com dois posicionais
    /// reais o parsing tem de dar certo, e o diretório não entra na lista
    /// de arquivos.
    #[test]
    fn cli_valor_de_catalogos_nao_vira_posicional() {
        let cfg = cfg_de(&["-c", "i18n", "s.toml", "a.toml"]);
        assert_eq!(cfg.arquivos, vec![PathBuf::from("a.toml")]);
    }

    #[test]
    fn cli_erros_de_uso() {
        assert_eq!(erro_de(&[]), ErroCli::FaltamArgs);
        assert_eq!(erro_de(&["s.toml"]), ErroCli::FaltamArgs);
        assert_eq!(erro_de(&["s.toml", "a.toml", "-l"]), ErroCli::LogSemCaminho);
        assert_eq!(erro_de(&["s.toml", "a.toml", "-i"]), ErroCli::IdiomaSemValor);
        assert_eq!(
            erro_de(&["-i", "tlh", "s.toml", "a.toml"]),
            ErroCli::IdiomaInvalido {
                idioma: "tlh".to_string(),
                disponiveis: vec!["en".to_string(), "pt".to_string()],
            }
        );
    }

    /// Idioma inválido é detectado antes de os posicionais serem contados:
    /// reclamar de "faltam argumentos" aqui esconderia a causa real.
    #[test]
    fn cli_idioma_invalido_tem_prioridade_sobre_faltam_args() {
        assert!(matches!(
            erro_de(&["-i", "tlh"]),
            ErroCli::IdiomaInvalido { .. }
        ));
    }

    #[test]
    fn cli_help_e_idiomas_saem_sem_validar() {
        let idiomas = Idiomas::carregar(None);
        for forma in [vec!["-h"], vec!["--help"], vec!["s.toml", "a.toml", "-h"]] {
            let acao = parse_cli(&args(&forma), &idiomas, "en").expect("não deveria falhar");
            assert!(matches!(acao, Acao::Imprimir(_)), "{forma:?}");
        }
        match parse_cli(&args(&["--idiomas"]), &idiomas, "en") {
            Ok(Acao::Imprimir(t)) => assert_eq!(t, "en\npt"),
            outro => panic!("esperava a lista de idiomas, obtive {outro:?}"),
        }
    }

    #[test]
    fn dir_catalogos_pega_a_ultima_ocorrencia() {
        assert_eq!(dir_catalogos(&args(&["s.toml"])), None);
        assert_eq!(
            dir_catalogos(&args(&["-c", "a", "--catalogos", "b", "s.toml"])),
            Some(PathBuf::from("b"))
        );
        // Sem valor após a flag, não há o que carregar.
        assert_eq!(dir_catalogos(&args(&["s.toml", "-c"])), None);
    }

    // --- schema ----------------------------------------------------

    #[test]
    fn carregar_schema_devolve_a_versao_do_nome_do_arquivo() {
        let (_, versao) = carregar_schema(&fixture("elemento-requisito-v1.toml")).unwrap();
        assert_eq!(versao, "elemento-requisito-v1");
    }

    #[test]
    fn carregar_schema_distingue_ilegivel_de_invalido() {
        assert!(matches!(
            carregar_schema(&fixture("nao-existe.toml")),
            Err(ErroSchema::Ilegivel { .. })
        ));
        assert!(matches!(
            carregar_schema(&fixture("schema-quebrado.toml")),
            Err(ErroSchema::Invalido { .. })
        ));
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

    // --- leitura de campo texto ------------------------------------

    #[test]
    fn ler_campo_texto_cobre_os_tres_casos() {
        let tabela: toml::value::Table =
            toml::from_str("presente = \"ok\"\nnumero = 42\n").unwrap();

        assert_eq!(
            ler_campo_texto(&tabela, "presente", Msg::TipoAusente).unwrap(),
            "ok"
        );

        let d = ler_campo_texto(&tabela, "numero", Msg::TipoAusente).unwrap_err();
        assert_eq!(d.msg.chave_e_args().0, "esperado-texto");
        assert!(matches!(d.local, Local::Campo(ref c) if c == "numero"));

        // A mensagem de ausência é a que o chamador passou.
        let d = ler_campo_texto(&tabela, "sumido", Msg::SchemaIdAusente).unwrap_err();
        assert_eq!(d.msg.chave_e_args().0, "schema-id-ausente");
    }

    // --- validação -------------------------------------------------

    #[test]
    fn exemplos_validos_passam() {
        let schema = schema_de_teste();
        for nome in ["exemplo-pt.toml", "exemplo-en.toml"] {
            let rel = validar_arquivo(&schema, "elemento-requisito-v1", &fixture(nome));
            assert!(
                rel.valido(),
                "{nome} deveria passar; erros: {:?}",
                rel.chaves()
            );
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

        let chaves = rel.chaves();

        for esperada in [
            "alias-de-outro-idioma",        // "name" num arquivo pt
            "enum-invalido-outro-idioma",   // prioridade = "high"
            "obrigatorio-ausente",          // fluxo_principal
            "esperado-texto",               // gatilho = 42 / item numérico
            "campo-desconhecido",           // "cor"
            "subcampo-obrigatorio-ausente", // relacoes[0].alvo
        ] {
            assert!(
                chaves.contains(&esperada),
                "esperava '{esperada}' entre os diagnósticos; obtive {chaves:?}"
            );
        }
    }

    /// O caminho reportado dentro de subtabela precisa manter o prefixo do
    /// item — é o que o autor vai procurar no arquivo dele.
    #[test]
    fn diagnostico_de_subcampo_leva_o_prefixo_do_item() {
        let schema = schema_de_teste();
        let rel = validar_arquivo(
            &schema,
            "elemento-requisito-v1",
            &fixture("exemplo-invalido.toml"),
        );

        let locais: Vec<String> = rel
            .diagnosticos
            .iter()
            .filter_map(|d| match &d.local {
                Local::Campo(c) => Some(c.clone()),
                _ => None,
            })
            .collect();

        assert!(
            locais.iter().any(|c| c == "relacoes[0].alvo"),
            "esperava 'relacoes[0].alvo' entre os locais; obtive {locais:?}"
        );
    }

    /// Raiz e subtabela passam pela mesma função; o que muda é a família de
    /// mensagens. Um campo desconhecido tem de sair como "campo" na raiz e
    /// "subcampo" dentro do item.
    #[test]
    fn campo_desconhecido_muda_de_familia_conforme_o_nivel() {
        let schema = schema_de_teste();
        let rel = validar_arquivo(
            &schema,
            "elemento-requisito-v1",
            &fixture("exemplo-invalido.toml"),
        );
        let chaves = rel.chaves();
        assert!(chaves.contains(&"campo-desconhecido"));
        assert!(chaves.contains(&"subcampo-desconhecido"));
    }

    #[test]
    fn arquivo_inexistente_nao_entra_em_panico() {
        let schema = schema_de_teste();
        let rel = validar_arquivo(&schema, "elemento-requisito-v1", &fixture("nao-existe.toml"));
        assert!(!rel.valido());
        assert_eq!(rel.chaves()[0], "arquivo-ilegivel");
    }

    // --- log -------------------------------------------------------

    #[test]
    fn log_traz_cabecalho_blocos_e_resumo() {
        let idiomas = Idiomas::carregar(None);
        let (schema, versao) = carregar_schema(&fixture("elemento-requisito-v1.toml")).unwrap();
        let arquivos = vec![fixture("exemplo-pt.toml"), fixture("exemplo-invalido.toml")];
        let cfg = Config {
            caminho_schema: fixture("elemento-requisito-v1.toml"),
            arquivos: arquivos.clone(),
            caminho_log: None,
            auto: false,
            l_geral: "pt".to_string(),
            padrao: "pt".to_string(),
        };

        let resultados = executar_validacao(&schema, &versao, &arquivos);
        let log = montar_log(&cfg, &resultados, &idiomas);

        assert!(log.contains(&idiomas.rot("pt", "titulo")));
        assert!(log.contains(&idiomas.rot("pt", "resumo")));
        assert!(log.contains("[1/2]"));
        assert!(log.contains("[2/2]"));
        assert!(log.contains(&idiomas.rot("pt", "ok")));
        assert!(log.contains(&idiomas.rot("pt", "falha")));
        // Nenhuma chave de catálogo escapou para a saída.
        assert!(!log.contains('⟨'), "chave ausente no log:\n{log}");
    }

    /// Em `--idioma auto` o bloco de cada arquivo segue o idioma que o
    /// próprio arquivo declara, sem revalidação.
    #[test]
    fn log_em_auto_usa_o_idioma_declarado_por_arquivo() {
        let idiomas = Idiomas::carregar(None);
        let (schema, versao) = carregar_schema(&fixture("elemento-requisito-v1.toml")).unwrap();
        let arquivos = vec![fixture("exemplo-pt.toml"), fixture("exemplo-en.toml")];
        let cfg = Config {
            caminho_schema: fixture("elemento-requisito-v1.toml"),
            arquivos: arquivos.clone(),
            caminho_log: None,
            auto: true,
            l_geral: "en".to_string(),
            padrao: "en".to_string(),
        };

        let resultados = executar_validacao(&schema, &versao, &arquivos);
        let log = montar_log(&cfg, &resultados, &idiomas);

        assert!(log.contains("auto"));
        assert!(log.contains(&idiomas.rot("pt", "ok")));
        assert!(log.contains(&idiomas.rot("en", "ok")));
    }
}
