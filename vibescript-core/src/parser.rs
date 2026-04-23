use crate::ast::*;
use crate::lexer::Token;
use crate::value::Value;
use chumsky::prelude::*;

pub fn parser<'a>() -> impl Parser<'a, &'a [Token], Vec<Stmt>, extra::Err<Rich<'a, Token>>> {
    stmt_parser()
        .repeated()
        .collect::<Vec<_>>()
        .then_ignore(end())
}
fn stmt_parser<'a>() -> impl Parser<'a, &'a [Token], Stmt, extra::Err<Rich<'a, Token>>> {
    recursive(|stmt| {
        let block_lit = just(Token::Do)
            .ignore_then(
                select! { Token::Ident(name) => name }
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::Pipe), just(Token::Pipe))
                    .or(empty().to(vec![])),
            )
            .then(stmt.clone().repeated().collect::<Vec<_>>())
            .then_ignore(just(Token::End))
            .map(|(params, body)| Expr::Block { params, body });
        let expr = expr_parser(block_lit);
        let block = stmt.repeated().collect::<Vec<_>>();

        let if_stmt = just(Token::If)
            .ignore_then(expr.clone())
            .then(block.clone())
            .then(
                just(Token::Elsif)
                    .ignore_then(expr.clone())
                    .then(block.clone())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .then(just(Token::Else).ignore_then(block.clone()).or_not())
            .then_ignore(just(Token::End))
            .map(
                |(((condition, then_branch), elsif_branches), else_branch)| Stmt::If {
                    condition,
                    then_branch,
                    elsif_branches,
                    else_branch,
                },
            );
        let while_stmt = just(Token::While)
            .ignore_then(expr.clone())
            .then(block.clone())
            .then_ignore(just(Token::End))
            .map(|(condition, body)| Stmt::While { condition, body });

        let until_stmt = just(Token::Until)
            .ignore_then(expr.clone())
            .then(block.clone())
            .then_ignore(just(Token::End))
            .map(|(condition, body)| Stmt::Until { condition, body });

        let for_stmt = just(Token::For)
            .ignore_then(select! { Token::Ident(name) => name })
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .then(block.clone())
            .then_ignore(just(Token::End))
            .map(|((var, iterable), body)| Stmt::For {
                var,
                iterable,
                body,
            });

        let def_stmt = just(Token::Def)
            .ignore_then(select! { Token::Ident(name) => name })
            .then(
                select! { Token::Ident(name) => name }
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .or(empty().to(vec![])),
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

        let break_stmt = just(Token::Break).to(Stmt::Break);
        let next_stmt = just(Token::Next).to(Stmt::Next);

        let return_stmt = just(Token::Return)
            .ignore_then(expr.clone().or_not())
            .map(Stmt::Return);

        let begin_stmt = just(Token::Begin)
            .ignore_then(block.clone())
            .then(
                just(Token::Rescue)
                    .ignore_then(
                        select! { Token::Ident(name) => name }
                            .separated_by(just(Token::Comma))
                            .collect::<Vec<_>>()
                            .delimited_by(just(Token::LParen), just(Token::RParen))
                            .or_not(),
                    )
                    .then(block.clone())
                    .map(|(types, body)| RescueClause {
                        types: types.unwrap_or_default(),
                        body,
                    })
                    .or_not(),
            )
            .then(just(Token::Ensure).ignore_then(block.clone()).or_not())
            .then_ignore(just(Token::End))
            .map(|((body, rescue), ensure)| Stmt::Try {
                body,
                rescue,
                ensure,
            });
        choice((
            if_stmt,
            while_stmt,
            until_stmt,
            for_stmt,
            def_stmt,
            return_stmt,
            break_stmt,
            next_stmt,
            begin_stmt,
            assignment,
            expr.clone().map(Stmt::Expression),
        ))
    })
    .boxed()
}

fn expr_parser<'a>(
    block_lit: impl Parser<'a, &'a [Token], Expr, extra::Err<Rich<'a, Token>>> + Clone + 'a,
) -> impl Parser<'a, &'a [Token], Expr, extra::Err<Rich<'a, Token>>> + Clone {
    recursive(|expr| {
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
        let array = expr
            .clone()
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
        let atom = choice((
            literal,
            variable,
            array,
            hash,
            block_lit.clone(),
            expr.clone()
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        ));
        let call_or_index_or_member = atom.clone().foldl(
            choice((
                expr.clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .then(block_lit.clone().or_not())
                    .map(|(args, block)| ("call", args, String::new(), block)),
                expr.clone()
                    .delimited_by(just(Token::LBracket), just(Token::RBracket))
                    .map(|idx| ("index", vec![idx], String::new(), None)),
                just(Token::Dot)
                    .ignore_then(select! { Token::Ident(name) => name })
                    .then(
                        expr.clone()
                            .separated_by(just(Token::Comma))
                            .allow_trailing()
                            .collect::<Vec<_>>()
                            .delimited_by(just(Token::LParen), just(Token::RParen))
                            .or_not(),
                    )
                    .then(block_lit.clone().or_not())
                    .map(|((name, args), block)| ("member", args.unwrap_or_default(), name, block)),
            ))
            .repeated(),
            |lhs, (kind, args, name, block)| match kind {
                "call" => {
                    if let Expr::Variable(func_name) = lhs {
                        Expr::Call {
                            func: func_name,
                            args,
                            kwargs: vec![],
                            block: block.map(Box::new),
                        }
                    } else {
                        lhs
                    }
                }
                "index" => Expr::Binary {
                    left: Box::new(lhs),
                    op: BinaryOp::Index,
                    right: Box::new(args[0].clone()),
                },
                "member" => Expr::Member {
                    receiver: Box::new(lhs),
                    method: name,
                    args,
                    block: block.map(Box::new),
                },
                _ => unreachable!(),
            },
        );
        let unary = just(Token::Minus)
            .to(UnaryOp::Neg)
            .or(just(Token::Not).to(UnaryOp::Not))
            .repeated()
            .foldr(call_or_index_or_member, |op, expr| Expr::Unary {
                op,
                expr: Box::new(expr),
            });
        let mul_div_mod = unary.clone().foldl(
            choice((
                just(Token::Star).to(BinaryOp::Mul),
                just(Token::Slash).to(BinaryOp::Div),
                just(Token::Percent).to(BinaryOp::Modulo),
            ))
            .then(unary.clone())
            .repeated(),
            |lhs, (op, rhs)| Expr::Binary {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
            },
        );
        let add_sub = mul_div_mod.clone().foldl(
            choice((
                just(Token::Plus).to(BinaryOp::Add),
                just(Token::Minus).to(BinaryOp::Sub),
            ))
            .then(mul_div_mod.clone())
            .repeated(),
            |lhs, (op, rhs)| Expr::Binary {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
            },
        );
        let comparison = add_sub.clone().foldl(
            choice((
                just(Token::Eq).to(BinaryOp::Eq),
                just(Token::NotEq).to(BinaryOp::NotEq),
                just(Token::Lt).to(BinaryOp::Lt),
                just(Token::LtEq).to(BinaryOp::LtEq),
                just(Token::Gt).to(BinaryOp::Gt),
                just(Token::GtEq).to(BinaryOp::GtEq),
            ))
            .then(add_sub.clone())
            .repeated(),
            |lhs, (op, rhs)| Expr::Binary {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
            },
        );
        let logical_and = comparison.clone().foldl(
            just(Token::And)
                .to(BinaryOp::And)
                .then(comparison.clone())
                .repeated(),
            |lhs, (op, rhs)| Expr::Binary {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
            },
        );
        let logical_or = logical_and.clone().foldl(
            just(Token::Or)
                .to(BinaryOp::Or)
                .then(logical_and.clone())
                .repeated(),
            |lhs, (op, rhs)| Expr::Binary {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
            },
        );
        logical_or
    })
    .boxed()
}
