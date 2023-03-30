use eyre::eyre;
use phf::phf_map;
use rnix::ast::{Attr, Attrpath, Entry, Expr, HasEntry, Param};
use std::collections::HashMap;

#[derive(Clone, Debug)]
enum NixObject {
    Set(NixSet),
    Nixpkg(String),
    FormatFactory { format_type: String },
}

impl NixObject {
    fn try_into_set(self) -> color_eyre::Result<NixSet> {
        match self {
            Self::Set(s) => Ok(s),
            Self::Nixpkg(pkg) => Err(eyre!("nixpkg '{}' cannot be treated as set", pkg)),
            Self::FormatFactory { format_type } => Err(eyre!(
                "'{}' format factory cannot be treated as set",
                format_type
            )),
        }
    }

    fn apply(self, arg: NixObject) -> color_eyre::Result<NixObject> {
        Err(eyre!("cannot apply lambda of type {:#?}", self))
    }
}

#[derive(Clone, Debug)]
enum NixSet {
    Dyn(HashMap<String, NixObject>),
    CallpkgArgs,
    Lib,
    Nixpkgs,
    Config,
    ConfigVal(Vec<String>),
    PkgsFormats,
}

static CALLPACKAGE_ARGS: phf::Map<&'static str, NixSet> = phf_map! {
    "lib" => NixSet::Lib,
    "pkgs" => NixSet::Nixpkgs,
    "config" => NixSet::Config,
};

static LIB: phf::Map<&'static str, NixSet> = phf_map! {};

fn lookup_nixpkg(name: &str) -> NixObject {
    if name == "formats" {
        return NixObject::Set(NixSet::PkgsFormats);
    }
    NixObject::Nixpkg(name.to_owned())
}

impl NixSet {
    fn lookup(&self, k: &str) -> Option<NixObject> {
        let h = |m: &phf::Map<&'static str, NixSet>| m.get(k).map(|v| NixObject::Set(v.clone()));
        match self {
            Self::Dyn(s) => s.get(k).cloned(),
            Self::CallpkgArgs => h(&CALLPACKAGE_ARGS),
            Self::Lib => h(&LIB),
            Self::Nixpkgs => Some(lookup_nixpkg(k)),
            Self::Config => Some(NixObject::Set(NixSet::ConfigVal(vec![k.to_owned()]))),
            Self::ConfigVal(path) => Some(NixObject::Set(NixSet::ConfigVal(
                path.iter()
                    .cloned()
                    .chain(std::iter::once(k.to_owned()))
                    .collect(),
            ))),
            // https://github.com/NixOS/nixpkgs/blob/master/pkgs/pkgs-lib/formats.nix
            Self::PkgsFormats => Some(NixObject::FormatFactory {
                format_type: k.to_owned(),
            }),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Scope {
    items: HashMap<String, NixObject>,
    with_namespaces: Vec<NixSet>,
}

impl Scope {
    fn new() -> Self {
        Default::default()
    }

    // We handle precedence of items vs. with by promoting items itno a with_namespaces
    //  entry every time a new with_namespace is added.
    fn lookup(&self, k: &str) -> Option<NixObject> {
        if let Some(obj) = self.items.get(k) {
            return Some(obj.clone());
        }
        for namespace in self.with_namespaces.iter() {
            if let Some(obj) = namespace.lookup(k) {
                return Some(obj);
            }
        }
        None
    }
}

fn token_type(expr: &Expr) -> &'static str {
    match expr {
        Expr::Apply(_) => "apply",
        Expr::Assert(_) => "assert",
        Expr::Error(_) => "error",
        Expr::IfElse(_) => "ifelse",
        Expr::Select(_) => "select",
        Expr::Str(_) => "str",
        Expr::Path(_) => "path",
        Expr::Literal(_) => "literal",
        Expr::Lambda(_) => "lambda",
        Expr::LegacyLet(_) => "legacylet",
        Expr::LetIn(_) => "letin",
        Expr::List(_) => "list",
        Expr::BinOp(_) => "binop",
        Expr::Paren(_) => "paren",
        Expr::Root(_) => "root",
        Expr::AttrSet(_) => "attrset",
        Expr::UnaryOp(_) => "unaryop",
        Expr::Ident(_) => "ident",
        Expr::With(_) => "with",
        Expr::HasAttr(_) => "hasattr",
    }
}

fn eval_object(scope: &Scope, expr: Expr) -> color_eyre::Result<NixObject> {
    match expr {
        Expr::With(with) => {
            let namespace = with.namespace().ok_or(eyre!("with has no namespace"))?;
            let mut new_scope = scope.clone();
            let old_items = std::mem::replace(&mut new_scope.items, HashMap::new());
            new_scope.with_namespaces.push(NixSet::Dyn(old_items));
            new_scope
                .with_namespaces
                .push(eval_object(scope, namespace)?.try_into_set()?);
            eval_object(&new_scope, with.body().ok_or(eyre!("with has no body"))?)
        }
        Expr::LetIn(letin) => {
            let mut new_scope = scope.clone();
            for entry in letin.entries().into_iter() {
                match entry {
                    Entry::AttrpathValue(attrval) => {
                        let attrs = attrval
                            .attrpath()
                            .ok_or(eyre!("let entry without attrpath"))?
                            .attrs()
                            .collect::<Vec<_>>();
                        if attrs.len() != 1 {
                            return Err(eyre!(
                                "expect single attr in let attrpath, found {}",
                                attrs.len()
                            ));
                        }
                        let Attr::Ident(ident) = attrs.first().unwrap() else {
                            return Err(eyre!("unexpected let attr type"));
                        };
                        let val = attrval.value().ok_or(eyre!("let binding without value"))?;
                        new_scope
                            .items
                            .insert(ident.to_string(), eval_object(&new_scope, val)?);
                    }
                    Entry::Inherit(inherit) => {
                        todo!();
                    }
                }
            }
            eval_object(scope, letin.body().ok_or(eyre!("letin without body"))?)
        }
        Expr::AttrSet(set) => {
            let is_rec = set.rec_token().is_some();
            let mut new_scope = scope.clone();
            let mut set_vals = HashMap::<String, NixObject>::new();
            for entry in set.entries().into_iter() {
                println!("{:#?}", entry);
                match entry {
                    Entry::AttrpathValue(attrval) => {
                        let val = eval_object(
                            &new_scope,
                            attrval.value().ok_or(eyre!("set entry without value"))?,
                        )?;
                        let entry: Option<
                            std::collections::hash_map::Entry<'_, String, NixObject>,
                        > = None;
                        for attr in attrval
                            .attrpath()
                            .ok_or(eyre!("set entry without attrpath"))?
                            .attrs()
                            .into_iter()
                        {
                            let Attr::Ident(attr_ident) = attr else {
                                return Err(eyre!("unimplemented"));
                            };
                            let attr_ident = attr_ident.to_string();
                            entry = match entry {
                                None => Some(set_vals.entry(attr_ident)),
                                Some(entry) => {
                                    let NixObject::Set(s) = entry.or_insert_with(|| NixObject::Set(NixSet::Dyn(HashMap::new()))) else {
                                        return Err(eyre!("cannot access attributes of non-set value"));
                                    };
                                }
                            };
                        }
                    }
                    _ => todo!(),
                }
            }
            todo!();
        }
        Expr::Ident(ident) => scope
            .lookup(ident.to_string().as_ref())
            .ok_or(eyre!("value not in scope: {}", ident)),
        Expr::Select(s) => {
            println!("{:#?}", s);
            let initial = eval_object(scope, s.expr().ok_or(eyre!("select without expr"))?)?;
            s.attrpath()
                .ok_or(eyre!("select without attrpath"))?
                .attrs()
                .into_iter()
                .try_fold::<_, _, color_eyre::Result<_>>(initial, |prev, attr| match attr {
                    Attr::Ident(i) => prev
                        .clone()
                        .try_into_set()?
                        .lookup(i.to_string().as_ref())
                        .ok_or(eyre!("value not in scope: {}", i)),
                    Attr::Dynamic(_) => todo!(),
                    Attr::Str(_) => todo!(),
                })
        }
        Expr::Apply(a) => {
            let lambda = eval_object(scope, a.lambda().ok_or(eyre!("apply without lambda"))?)?;
            let argument =
                eval_object(scope, a.argument().ok_or(eyre!("apply without argument"))?)?;
            lambda.apply(argument)
        }
        expr => return Err(eyre!("cannot eval object of type: {}", token_type(&expr))),
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let input = include_str!("../test.nix.txt");
    let ast = rnix::Root::parse(&input).ok()?;
    let expr = ast.expr().ok_or(eyre!("file is empty"))?;
    let Expr::Lambda(lambda) = expr else {
        return Err(eyre!("file does not contain a lambda"));
    };

    let mut scope = Scope::new();
    let param = lambda
        .param()
        .ok_or(eyre!("top-level lambda does not have a param"))?;
    let Param::Pattern(pat) = param else {
        return Err(eyre!("top-level lambda does not destructure its argument"));
    };
    if let Some(bind) = pat.pat_bind() {
        scope.items.insert(
            bind.ident().ok_or(eyre!("bind without ident"))?.to_string(),
            NixObject::Set(NixSet::CallpkgArgs),
        );
    }
    for e in pat.pat_entries() {
        let ident = e
            .ident()
            .ok_or(eyre!("pat entry without ident"))?
            .to_string();
        let val = NixObject::Set(
            CALLPACKAGE_ARGS
                .get(&ident)
                .ok_or(eyre!("unknown callPackage arg: {}", ident))?
                .clone(),
        );
        scope.items.insert(ident, val);
    }

    let body = lambda.body().ok_or(eyre!("lambda without body"))?;
    eval_object(&scope, body)?;
    Ok(())
}
