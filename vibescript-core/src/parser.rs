use crate::ast::*;
use crate::lexer::Token;
use crate::value::Value;
use chumsky::prelude::*;

pub fn parser<'a>() -> impl Parser<'a, &'a [Token], Vec<Stmt>, extra::Err<Rich<'a, Token>>> {
    recursive(|stmt| {
        let expr = recursive(|expr| {
            let literal = select! {
                Token::Int(i) => Expr::Literal(Value::Int(i)),
                Token::Float(f) => Expr::Literal(Value::Float(f)),
                Token::String(s) => Expr::Literal(Value::String(s)),
                Token::True => Expr::Literal(Value::Bool(true)),
                Token::False => Expr::Literal(Value::Bool(false)),
                Token::Nil => Expr::Literal(Value::Nil),
            };

            let variable = select! {
                Token::Ident(name) => Expr::Variable(name),
            };

            let array = expr.clone()
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBracket), just(Token::RBracket))
                .map(Expr::Array);

            let hash = select! { Token::Ident(name) => name }
                .or(select! { Token::String(s) => s })
                .then_ignore(just(Token::Colon))
                .then(expr.clone())
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace))
                .map(Expr::Hash);

            let atom = literal
                .or(variable)
                .or(array)
                .or(hash)
                .or(expr.clone().delimited_by(just(Token::LParen), just(Token::RParen)));

            let call_or_index = atom.clone().foldl(
                choice((
                    expr.clone()
                        .separated_by(just(Token::Comma))
                        .allow_trailing()
                        .collect::<Vec<_>>()
                        .delimited_by(just(Token::LParen), just(Token::RParen))
                        .map(|args| ("call", args)),
                    expr.clone()
                        .delimited_by(just(Token::LBracket), just(Token::RBracket))
                        .map(|idx| ("index", vec![idx])),
                )).repeated(),
                |lhs, (kind, args)| {
                    match kind {
                        "call" => {
                            if let Expr::Variable(name) = lhs {
                                Expr::Call { func: name, args, kwargs: vec![] }
                            } else {
                                lhs // Simplified for MVP
                            }
                        }
                        "index" => {
                             Expr::Binary {
                                left: Box::new(lhs),
                                op: BinaryOp::Index,
                                right: Box::new(args[0].clone()),
                            }
                        }
                        _ => unreachable!()
                    }
                }
            );

            let unary = just(Token::Minus)
                .to(UnaryOp::Neg)
                .or(just(Token::Not).to(UnaryOp::Not))
                .repeated()
                .foldr(call_or_index, |op, expr| Expr::Unary {
                    op,
                    expr: Box::new(expr),
                });

            let mul_div = unary.clone().foldl(
                just(Token::Star)
                    .to(BinaryOp::Mul)
                    .or(just(Token::Slash).to(BinaryOp::Div))
                    .then(unary.clone())
                    .repeated(),
                |lhs, (op, rhs)| Expr::Binary {
                    left: Box::new(lhs),
                    op,
                    right: Box::new(rhs),
                },
            );

            let add_sub = mul_div.clone().foldl(
                just(Token::Plus)
                    .to(BinaryOp::Add)
                    .or(just(Token::Minus).to(BinaryOp::Sub))
                    .then(mul_div.clone())
                    .repeated(),
                |lhs, (op, rhs)| Expr::Binary {
                    left: Box::new(lhs),
                    op,
                    right: Box::new(rhs),
                },
            );

            let comparison = add_sub.clone().foldl(
                just(Token::Eq).to(BinaryOp::Eq)
                    .or(just(Token::NotEq).to(BinaryOp::NotEq))
                    .or(just(Token::Lt).to(BinaryOp::Lt))
                    .or(just(Token::LtEq).to(BinaryOp::LtEq))
                    .or(just(Token::Gt).to(BinaryOp::Gt))
                    .or(just(Token::GtEq).to(BinaryOp::GtEq))
                    .then(add_sub.clone())
                    .repeated(),
                |lhs, (op, rhs)| Expr::Binary {
                    left: Box::new(lhs),
                    op,
                    right: Box::new(rhs),
                }
            );

            comparison
        });

        let block = stmt.repeated().collect::<Vec<_>>();

        let if_stmt = just(Token::If)
            .ignore_then(expr.clone())
            .then(block.clone())
            .then(just(Token::Else).ignore_then(block.clone()).or_not())
            .then_ignore(just(Token::End))
            .map(|((condition, then_branch), else_branch)| Stmt::If {
                condition,
                then_branch,
                else_branch,
            });

        let while_stmt = just(Token::While)
            .ignore_then(expr.clone())
            .then(block.clone())
            .then_ignore(just(Token::End))
            .map(|(condition, body)| Stmt::While { condition, body });

        let def_stmt = just(Token::Def)
            .ignore_then(select! { Token::Ident(name) => name })
            .then(
                select! { Token::Ident(name) => name }
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .or(empty().to(vec![]))
            )
            .then(block.clone())
            .then_ignore(just(Token::End))
            .map(|((name, params), body)| Stmt::Function { name, params, body });

        let assignment = select! {
            Token::Ident(name) => name,
        }
        .then_ignore(just(Token::Assign))
        .then(expr.clone())
        .map(|(name, value)| Stmt::Assignment { name, value });

        let return_stmt = just(Token::Return)
            .ignore_then(expr.clone().or_not())
            .map(Stmt::Return);

        if_stmt
            .or(while_stmt)
            .or(def_stmt)
            .or(return_stmt)
            .or(assignment)
            .or(expr.clone().map(Stmt::Expression))
    })
    .repeated()
    .collect::<Vec<_>>()
    .then_ignore(end())
}
