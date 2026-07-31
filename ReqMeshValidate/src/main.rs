//! reqMeshVal — validador de Elementos de Requisito (ReqMesh).
//!
//! Uso:
//!     reqMeshVal <schema> <arquivo1> [arquivo2 ...] [--log <caminho>]
//!
//! Lê o arquivo de schema (ex: elemento-requisito-v1.toml) e valida cada
//! arquivo de Elemento de Requisito contra ele, emitindo um log por arquivo.
//!
//! Regras aplicadas (nesta ordem, por arquivo):
//!   1. `schema_id` deve existir e ser texto. É o único campo fixo/não
//!      localizado — é ele que diz qual idioma usar para ler o resto.
//!   2. `schema_id` = "<versao>;<locale>". Sem ";" ou com locale vazio,
//!      o idioma assumido é "en". Do locale usa-se só o idioma-base
//!      (pt_BR -> pt); a região não distingue tabelas de alias.
//!   3. O campo discriminador `tipo` é resolvido primeiro, porque é ele
//!      que define quais campos específicos passam a valer (caso de uso
//!      ou ator).
//!   4. Toda chave do arquivo precisa corresponder a um alias do idioma
//!      declarado. Alias de OUTRO idioma é erro, não campo desconhecido
//!      genérico — é o que impede mistura de idiomas no mesmo arquivo.
//!   5. Tipos de dado e subcampos são verificados; por fim, campos
//!      obrigatórios ausentes são reportados.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use toml::Value;

/// Único campo fixo, nunca localizado (necessário para bootstrap).
const CAMPO_SCHEMA_ID: &str = "schema_id";
/// Idioma assumido quando o locale vem vazio ou ausente.
const IDIOMA_PADRAO: &str = "en";
/// Nome canônico do campo discriminador de tipo do elemento.
const CAMPO_TIPO: &str = "tipo";

const USO: &str = "\
reqMeshVal — validador de Elementos de Requisito (ReqMesh)

USO:
    reqMeshVal <schema> <arquivo1> [arquivo2 ...] [opções]

ARGUMENTOS:
    <schema>      arquivo de schema (ex: elemento-requisito-v1.toml)
    <arquivoN>    um ou mais arquivos de Elemento de Requisito

OPÇÕES:
    -l, --log <caminho>   grava o log também no arquivo indicado
    -h, --help            mostra esta ajuda

SAÍDA:
    0  todos os arquivos válidos (avisos não reprovam)
    1  pelo menos um arquivo com erro
    2  erro de uso ou falha ao carregar o schema";

// ============================================================
// Modelo do arquivo de schema
// ============================================================

#[derive(Debug, Deserialize)]
struct Schema {
    /// Campos comuns a todos os tipos de elemento.
    #[serde(default)]
    campos: BTreeMap<String, DefCampo>,
    /// Campos válidos apenas quando `tipo = caso_de_uso`.
    #[serde(default)]
    campos_caso_de_uso: BTreeMap<String, DefCampo>,
    /// Campos válidos apenas quando `tipo = ator`.
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
// Diagnósticos e relatório
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nivel {
    Erro,
    Aviso,
}

impl Nivel {
    fn etiqueta(self) -> &'static str {
        match self {
            Nivel::Erro => "ERRO ",
            Nivel::Aviso => "AVISO",
        }
    }
}

#[derive(Debug)]
struct Diagnostico {
    nivel: Nivel,
    local: String,
    mensagem: String,
}

impl Diagnostico {
    fn erro(local: impl Into<String>, mensagem: impl Into<String>) -> Self {
        Diagnostico {
            nivel: Nivel::Erro,
            local: local.into(),
            mensagem: mensagem.into(),
        }
    }

    fn aviso(local: impl Into<String>, mensagem: impl Into<String>) -> Self {
        Diagnostico {
            nivel: Nivel::Aviso,
            local: local.into(),
            mensagem: mensagem.into(),
        }
    }
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

    let mut posicionais: Vec<String> = Vec::new();
    let mut caminho_log: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("{USO}");
                return ExitCode::SUCCESS;
            }
            "-l" | "--log" => {
                i += 1;
                match args.get(i) {
                    Some(v) => caminho_log = Some(PathBuf::from(v)),
                    None => {
                        eprintln!("erro: --log exige um caminho\n\n{USO}");
                        return ExitCode::from(2);
                    }
                }
            }
            outro => posicionais.push(outro.to_string()),
        }
        i += 1;
    }

    if posicionais.len() < 2 {
        eprintln!("erro: informe o schema e ao menos um arquivo\n\n{USO}");
        return ExitCode::from(2);
    }

    let caminho_schema = PathBuf::from(&posicionais[0]);
    let arquivos: Vec<PathBuf> = posicionais[1..].iter().map(PathBuf::from).collect();

    // --- carrega o schema (falha aqui é fatal: sem schema não há validação) ---
    let bruto_schema = match fs::read_to_string(&caminho_schema) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "erro: não foi possível ler o schema '{}': {e}",
                caminho_schema.display()
            );
            return ExitCode::from(2);
        }
    };

    let schema: Schema = match toml::from_str(&bruto_schema) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "erro: schema '{}' inválido: {e}",
                caminho_schema.display()
            );
            return ExitCode::from(2);
        }
    };

    // A versão esperada vem do nome do arquivo de schema
    // (ex: elemento-requisito-v1.toml -> "elemento-requisito-v1").
    let versao_esperada = caminho_schema
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    // --- cabeçalho do log ---
    let mut log = String::new();
    log.push_str("=== reqMeshVal — relatório de validação ===\n");
    log.push_str(&format!("data    : {}\n", chrono::Local::now().to_rfc3339()));
    log.push_str(&format!("schema  : {}\n", caminho_schema.display()));
    log.push_str(&format!("versão  : {versao_esperada}\n"));
    log.push_str(&format!("arquivos: {}\n\n", arquivos.len()));

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

        log.push_str(&formatar_arquivo(idx + 1, total, arquivo, &rel));
    }

    // --- resumo ---
    log.push_str("--- resumo ---\n");
    log.push_str(&format!("arquivos : {total}\n"));
    log.push_str(&format!("válidos  : {validos}\n"));
    log.push_str(&format!("falhas   : {}\n", total - validos));
    log.push_str(&format!("erros    : {total_erros}\n"));
    log.push_str(&format!("avisos   : {total_avisos}\n"));

    print!("{log}");

    if let Some(caminho) = &caminho_log {
        if let Err(e) = fs::write(caminho, &log) {
            eprintln!(
                "aviso: não foi possível gravar o log em '{}': {e}",
                caminho.display()
            );
        }
    }

    if total_erros > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn formatar_arquivo(indice: usize, total: usize, caminho: &Path, rel: &Relatorio) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "[{indice}/{total}] {}\n",
        caminho.display()
    ));

    if let Some(sid) = &rel.schema_id {
        let idioma = rel.idioma.as_deref().unwrap_or("?");
        s.push_str(&format!(
            "         schema_id : {sid}  (idioma: {idioma})\n"
        ));
    }
    if let Some(tipo) = &rel.tipo {
        s.push_str(&format!("         tipo      : {tipo}\n"));
    }

    let erros = rel.erros();
    let avisos = rel.avisos();

    if rel.valido() {
        s.push_str(&format!(
            "         OK — {} campo(s) reconhecido(s), {avisos} aviso(s)\n",
            rel.campos_reconhecidos
        ));
    } else {
        s.push_str(&format!(
            "         FALHA — {erros} erro(s), {avisos} aviso(s)\n"
        ));
    }

    for d in &rel.diagnosticos {
        s.push_str(&format!(
            "           {} {:<28} {}\n",
            d.nivel.etiqueta(),
            d.local,
            d.mensagem
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
                "<arquivo>",
                format!("não foi possível ler o arquivo: {e}"),
            ));
            return rel;
        }
    };

    let valor: Value = match toml::from_str(&conteudo) {
        Ok(v) => v,
        Err(e) => {
            rel.diagnosticos
                .push(Diagnostico::erro("<toml>", format!("TOML inválido: {e}")));
            return rel;
        }
    };

    let tabela = match valor.as_table() {
        Some(t) => t,
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                "<toml>",
                "o arquivo não é uma tabela TOML de nível superior",
            ));
            return rel;
        }
    };

    // --- 1. schema_id (campo fixo, bootstrap) ---
    let bruto_id = match tabela.get(CAMPO_SCHEMA_ID) {
        Some(Value::String(s)) => s.clone(),
        Some(outro) => {
            rel.diagnosticos.push(Diagnostico::erro(
                CAMPO_SCHEMA_ID,
                format!("esperado texto, encontrado {}", outro.type_str()),
            ));
            return rel;
        }
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                CAMPO_SCHEMA_ID,
                "campo obrigatório ausente — é ele que define schema e idioma do arquivo",
            ));
            return rel;
        }
    };

    let (versao, idioma) = interpretar_schema_id(&bruto_id);
    rel.schema_id = Some(bruto_id.clone());
    rel.idioma = Some(idioma.clone());

    if versao != versao_esperada {
        rel.diagnosticos.push(Diagnostico::aviso(
            CAMPO_SCHEMA_ID,
            format!(
                "arquivo declara a versão '{versao}', mas o schema carregado é '{versao_esperada}'"
            ),
        ));
    }

    // --- 2. resolve o campo discriminador `tipo` ---
    let def_tipo = match schema.campos.get(CAMPO_TIPO) {
        Some(d) => d,
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                "<schema>",
                format!("o schema não define o campo canônico '{CAMPO_TIPO}'"),
            ));
            return rel;
        }
    };

    let alias_tipo = match def_tipo.alias.get(&idioma) {
        Some(a) => a.clone(),
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                "<schema>",
                format!("o schema não define alias de '{CAMPO_TIPO}' no idioma '{idioma}'"),
            ));
            return rel;
        }
    };

    let bruto_tipo = match tabela.get(&alias_tipo) {
        Some(Value::String(s)) => s.clone(),
        Some(outro) => {
            rel.diagnosticos.push(Diagnostico::erro(
                alias_tipo.clone(),
                format!("esperado texto, encontrado {}", outro.type_str()),
            ));
            return rel;
        }
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                alias_tipo.clone(),
                "campo obrigatório ausente — sem ele não é possível saber quais campos se aplicam",
            ));
            return rel;
        }
    };

    let tipo_canonico = match resolver_enum(def_tipo, &bruto_tipo, &idioma) {
        Some(c) => c,
        None => {
            rel.diagnosticos.push(Diagnostico::erro(
                alias_tipo.clone(),
                mensagem_enum_invalido(def_tipo, &bruto_tipo, &idioma),
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
            alias_tipo.clone(),
            format!("tipo '{outro}' não possui conjunto de campos específico no schema"),
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
                    Some((canonico, lang)) if lang != &idioma => format!(
                        "alias do idioma '{lang}' (campo canônico '{canonico}'), mas o arquivo \
                         declara idioma '{idioma}' — idiomas não podem ser misturados"
                    ),
                    _ => format!(
                        "campo desconhecido: não corresponde a nenhum campo do schema no idioma '{idioma}'"
                    ),
                };
                rel.diagnosticos.push(Diagnostico::erro(chave.as_str(), msg));
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
                nome,
                format!("campo obrigatório ausente (canônico: '{canonico}')"),
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
                    local,
                    format!("esperado texto, encontrado {}", valor.type_str()),
                ));
            }
        }

        "array<string>" => match valor.as_array() {
            None => diags.push(Diagnostico::erro(
                local,
                format!("esperado lista de textos, encontrado {}", valor.type_str()),
            )),
            Some(itens) => {
                if def.obrigatorio && itens.is_empty() {
                    diags.push(Diagnostico::erro(
                        local,
                        "campo obrigatório não pode ser uma lista vazia",
                    ));
                }
                for (i, item) in itens.iter().enumerate() {
                    if item.as_str().is_none() {
                        diags.push(Diagnostico::erro(
                            format!("{local}[{i}]"),
                            format!("esperado texto, encontrado {}", item.type_str()),
                        ));
                    }
                }
            }
        },

        "enum" => match valor.as_str() {
            None => diags.push(Diagnostico::erro(
                local,
                format!("esperado texto, encontrado {}", valor.type_str()),
            )),
            Some(s) => {
                if resolver_enum(def, s, idioma).is_none() {
                    diags.push(Diagnostico::erro(
                        local,
                        mensagem_enum_invalido(def, s, idioma),
                    ));
                }
            }
        },

        "array<tabela>" => match valor.as_array() {
            None => diags.push(Diagnostico::erro(
                local,
                format!("esperado lista de tabelas, encontrado {}", valor.type_str()),
            )),
            Some(itens) => {
                if def.obrigatorio && itens.is_empty() {
                    diags.push(Diagnostico::erro(
                        local,
                        "campo obrigatório não pode ser uma lista vazia",
                    ));
                }
                for (i, item) in itens.iter().enumerate() {
                    let local_i = format!("{local}[{i}]");
                    match item.as_table() {
                        None => diags.push(Diagnostico::erro(
                            local_i,
                            format!("esperado tabela, encontrado {}", item.type_str()),
                        )),
                        Some(t) => {
                            validar_subtabela(&def.subcampos, t, idioma, &local_i, diags)
                        }
                    }
                }
            }
        },

        outro => diags.push(Diagnostico::aviso(
            local,
            format!("schema declara tipo_dado desconhecido '{outro}' — campo não verificado"),
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
        diags.push(Diagnostico::aviso(
            local,
            "o schema não define subcampos para esta tabela — conteúdo não verificado",
        ));
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
                    Some((canonico, lang)) if lang.as_str() != idioma => format!(
                        "alias do idioma '{lang}' (subcampo canônico '{canonico}'), mas o arquivo \
                         declara idioma '{idioma}'"
                    ),
                    _ => format!("subcampo desconhecido no idioma '{idioma}'"),
                };
                diags.push(Diagnostico::erro(format!("{local}.{chave}"), msg));
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
                format!("{local}.{nome}"),
                format!("subcampo obrigatório ausente (canônico: '{canonico}')"),
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
        IDIOMA_PADRAO.to_string()
    } else {
        locale
            .split(['_', '-'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(IDIOMA_PADRAO)
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
                        "<schema>",
                        format!(
                            "alias '{a}' ({idioma}) é ambíguo: mapeia para '{anterior}' e '{canonico}'"
                        ),
                    ));
                }
            }
            None => diags.push(Diagnostico::aviso(
                "<schema>",
                format!(
                    "campo '{canonico}' não tem alias no idioma '{idioma}' — não pode ser usado neste arquivo"
                ),
            )),
        }
    }
    m
}

/// alias-em-qualquer-idioma -> (canônico, idioma). Usado só para gerar
/// mensagens de erro úteis quando o autor mistura idiomas.
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

fn mensagem_enum_invalido(def: &DefCampo, bruto: &str, idioma: &str) -> String {
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

    match em_outro_idioma {
        Some((canonico, lang)) => format!(
            "valor '{bruto}' pertence ao idioma '{lang}' (canônico '{canonico}'), mas o arquivo \
             declara '{idioma}'; válidos: {}",
            permitidos.join(", ")
        ),
        None => format!(
            "valor '{bruto}' inválido; válidos em '{idioma}': {}",
            permitidos.join(", ")
        ),
    }
}
