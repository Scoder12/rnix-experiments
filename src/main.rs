use eyre::eyre;
use rnix::ast::{Expr, Param};

fn main() -> color_eyre::Result<()> {
    let input = include_str!("../test.nix.txt");
    let ast = rnix::Root::parse(&input).ok()?;
    let expr = ast.expr().ok_or(eyre!("file is empty"))?;
    let Expr::Lambda(lambda) = expr else {
        return Err(eyre!("file does not contain a lambda"));
    };
    let param = lambda
        .param()
        .ok_or(eyre!("top-level lambda does not have a param"))?;
    let Param::Pattern(pat) = param else {
        return Err(eyre!("top-level lambda does not destructure its argument"));
    };
    println!("{:#?}", pat);
    Ok(())
}
