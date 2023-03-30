use eyre::eyre;
use rnix::ast::{Attr, AttrSet, Entry, Expr, HasEntry, Param};
use std::collections::HashMap;

fn proc_main_set(scope: &mut HashMap<String, String>, set: AttrSet) {
    println!("{set:#?}");
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

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let input = include_str!("../test.nix.txt");
    let ast = rnix::Root::parse(&input).ok()?;
    let expr = ast.expr().ok_or(eyre!("file is empty"))?;
    let Expr::Lambda(lambda) = expr else {
        return Err(eyre!("file does not contain a lambda"));
    };

    let mut scope: HashMap<String, String> = HashMap::new();
    let param = lambda
        .param()
        .ok_or(eyre!("top-level lambda does not have a param"))?;
    let Param::Pattern(pat) = param else {
        return Err(eyre!("top-level lambda does not destructure its argument"));
    };
    if let Some(bind) = pat.pat_bind() {
        scope.insert(
            bind.ident().ok_or(eyre!("bind without ident"))?.to_string(),
            "callPackage".to_string(),
        );
    }
    for e in pat.pat_entries() {
        let ident = e
            .ident()
            .ok_or(eyre!("pat entry without ident"))?
            .to_string();
        let val = format!("callPackage.{}", &ident);
        scope.insert(ident, val);
    }

    let mut body = lambda.body().ok_or(eyre!("lambda without body"))?;
    loop {
        match body {
            Expr::With(with) => {
                let Expr::Ident(namespace) =
                with.namespace().ok_or(eyre!("with has no namespace"))?
            else {
                return Err(eyre!("unexpected with namespace type"));
            };
                scope.insert(format!("{}.*", namespace.to_string()), "".to_owned());
                body = with.body().ok_or(eyre!("with has no body"))?;
            }
            Expr::LetIn(letin) => {
                for entry in letin.entries() {
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
                                return Err(eyre!("unexpect let attr type"));
                            };
                            let val = attrval.value();
                            todo!();
                        }
                        Entry::Inherit(inherit) => {
                            todo!();
                        }
                    }
                }
                body = letin.body().ok_or(eyre!("letin without body"))?;
            }
            Expr::AttrSet(set) => {
                proc_main_set(&mut scope, set);
                break;
            }
            _ => return Err(eyre!("unexpected lambda return type {:#?}", body)),
        }
    }

    println!("{:#?}", scope);
    Ok(())
}
