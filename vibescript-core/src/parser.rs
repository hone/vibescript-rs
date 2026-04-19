use crate::ast::*;
use crate::lexer::Token;
use crate::value::Value;
use chumsky::prelude::*;

pub fn parser<'a>() -> impl Parser<'a, &'a [Token], Vec<Stmt>, extra::Err<Rich<'a, Token>>> {
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

        let atom = literal
            .or(variable)
            .or(expr.delimited_by(just(Token::LParen), just(Token::RParen)));

        let unary = just(Token::Minus)
            .to(UnaryOp::Neg)
            .or(just(Token::Not).to(UnaryOp::Not))
            .repeated()
            .foldr(atom, |op, expr| Expr::Unary {
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

        add_sub
    });

    let assignment = select! {
        Token::Ident(name) => name,
    }
    .then_ignore(just(Token::Assign))
    .then(expr.clone())
    .map(|(name, value)| Stmt::Assignment { name, value });

    let stmt = assignment.or(expr.clone().map(Stmt::Expression));

    stmt.repeated().collect::<Vec<_>>().then_ignore(end())
}
