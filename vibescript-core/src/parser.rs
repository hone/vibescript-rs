use crate::ast::*;
use crate::lexer::Token;
use crate::value::{EnumMember, Param, Value};
use chumsky::prelude::*;

pub type ParserExtra<'a> = extra::Full<Rich<'a, Token>, (), ()>;

pub fn parser<'a>() -> impl Parser<'a, &'a [Token], Vec<Stmt>, ParserExtra<'a>> {
    stmt_parser()
        .repeated()
        .collect::<Vec<_>>()
        .then_ignore(end())
}

fn stmt_parser<'a>() -> impl Parser<'a, &'a [Token], Stmt, ParserExtra<'a>> {
    recursive(|stmt| {
        let name_parser = select! {
            Token::Ident(name) => name,
        }
        .boxed();

        let keyword_name_parser = choice((
            just(Token::In).to("in".to_string()),
            just(Token::Do).to("do".to_string()),
            just(Token::If).to("if".to_string()),
            just(Token::Else).to("else".to_string()),
            just(Token::Elsif).to("elsif".to_string()),
            just(Token::End).to("end".to_string()),
            just(Token::While).to("while".to_string()),
            just(Token::Until).to("until".to_string()),
            just(Token::For).to("for".to_string()),
            just(Token::Return).to("return".to_string()),
            just(Token::Assert).to("assert".to_string()),
            just(Token::True).to("true".to_string()),
            just(Token::False).to("false".to_string()),
            just(Token::Nil).to("nil".to_string()),
            just(Token::Then).to("then".to_string()),
            just(Token::Case).to("case".to_string()),
            just(Token::When).to("when".to_string()),
            just(Token::Begin).to("begin".to_string()),
            just(Token::Rescue).to("rescue".to_string()),
            just(Token::Ensure).to("ensure".to_string()),
            just(Token::Enum).to("enum".to_string()),
            just(Token::Class).to("class".to_string()),
            just(Token::SelfToken).to("self".to_string()),
            just(Token::Property).to("property".to_string()),
            just(Token::Getter).to("getter".to_string()),
            just(Token::Setter).to("setter".to_string()),
        ))
        .or(choice((
            just(Token::Private).to("private".to_string()),
            just(Token::Def).to("def".to_string()),
        )))
        .boxed();

        let member_name_parser = name_parser.clone().or(keyword_name_parser).boxed();

        let type_expr_parser = recursive(|type_expr| {
            let base_type = select! {
                Token::Ident(name) => {
                    let mut nullable = false;
                    let mut clean_name = name.clone();
                    if name.ends_with('?') {
                        nullable = true;
                        clean_name.pop();
                    }

                    let kind = match clean_name.as_str() {
                        "Any" | "any" => TypeKind::Any,
                        "Int" | "int" => TypeKind::Int,
                        "Float" | "float" => TypeKind::Float,
                        "Number" | "number" => TypeKind::Number,
                        "String" | "string" => TypeKind::String,
                        "Bool" | "bool" => TypeKind::Bool,
                        "Nil" | "nil" => TypeKind::Nil,
                        "Duration" | "duration" => TypeKind::Duration,
                        "Time" | "time" => TypeKind::Time,
                        "Money" | "money" => TypeKind::Money,
                        "Array" | "array" => TypeKind::Array,
                        "Hash" | "hash" => TypeKind::Hash,
                        "Function" | "function" => TypeKind::Function,
                        "Object" | "object" => TypeKind::Object,
                        _ => TypeKind::Enum,
                    };

                    TypeExpr {
                        name: clean_name,
                        kind,
                        nullable,
                        type_args: Vec::new(),
                        shape: Vec::new(),
                        union_types: Vec::new(),
                    }
                }
            }
            .then_ignore(just(Token::Dot).not());

            let generic_type = base_type
                .then(
                    type_expr
                        .clone()
                        .separated_by(just(Token::Comma))
                        .allow_trailing()
                        .collect::<Vec<_>>()
                        .delimited_by(just(Token::Lt), just(Token::Gt))
                        .or_not(),
                )
                .map(|(mut ty, args)| {
                    if let Some(args) = args {
                        ty.type_args = args;
                    }
                    ty
                });

            let shape_type = just(Token::LBrace)
                .ignore_then(
                    member_name_parser
                        .clone()
                        .then_ignore(just(Token::Colon))
                        .then(type_expr.clone())
                        .separated_by(just(Token::Comma))
                        .allow_trailing()
                        .collect::<Vec<_>>(),
                )
                .then_ignore(just(Token::RBrace))
                .map(|shape| TypeExpr {
                    name: "Shape".to_string(),
                    kind: TypeKind::Shape,
                    nullable: false,
                    type_args: Vec::new(),
                    shape,
                    union_types: Vec::new(),
                });

            let primary_type = choice((generic_type, shape_type));

            primary_type
                .then(
                    just(Token::Pipe)
                        .ignore_then(type_expr)
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .map(|(first, rest)| {
                    if rest.is_empty() {
                        first
                    } else {
                        let mut types = vec![first];
                        types.extend(rest);
                        TypeExpr {
                            name: "Union".to_string(),
                            kind: TypeKind::Union,
                            nullable: false,
                            type_args: Vec::new(),
                            shape: Vec::new(),
                            union_types: types,
                        }
                    }
                })
        })
        .boxed();

        let block_body = stmt.clone().repeated().collect::<Vec<_>>().boxed();

        let block_lit = just(Token::Do)
            .ignore_then(
                select! { Token::Ident(name) => name, Token::Ivar(name) => format!("@{}", name) }
                    .then(
                        just(Token::Colon)
                            .ignore_then(type_expr_parser.clone())
                            .or_not(),
                    )
                    .map(|(name, _type)| name)
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::Pipe), just(Token::Pipe))
                    .or(empty().to(vec![])),
            )
            .then(block_body.clone())
            .then_ignore(just(Token::End))
            .map(|(params, body)| Expr::Block { params, body })
            .boxed();

        let expr = recursive(|expr| {
            let string_parser = just(Token::StringStart)
                .ignore_then(
                    choice((
                        select! { Token::StringText(s) => StringPart::Text(s) },
                        just(Token::InterpolationStart)
                            .ignore_then(expr.clone())
                            .then_ignore(just(Token::InterpolationEnd))
                            .map(StringPart::Expr),
                    ))
                    .repeated()
                    .collect::<Vec<_>>(),
                )
                .then_ignore(just(Token::StringEnd))
                .map(|parts| {
                    if parts.len() == 1 {
                        if let StringPart::Text(s) = &parts[0] {
                            return Expr::Literal(Value::String(s.clone()));
                        }
                    }
                    if parts.is_empty() {
                        return Expr::Literal(Value::String(String::new()));
                    }
                    Expr::InterpolatedString(parts)
                })
                .boxed();

            let literal = select! {
                Token::Int(i) => Expr::Literal(Value::Int(i)),
                Token::Float(f) => Expr::Literal(Value::Float(f)),
                Token::Symbol(s) => Expr::Literal(Value::Symbol(s)),
                Token::True => Expr::Literal(Value::Bool(true)),
                Token::False => Expr::Literal(Value::Bool(false)),
                Token::Nil => Expr::Literal(Value::Nil),
            }
            .or(string_parser)
            .boxed();

            let variable = select! {
                Token::Ident(name) => Expr::Variable(name),
                Token::Ivar(name) => Expr::InstanceVar(name),
                Token::Cvar(name) => Expr::ClassVar(name),
            }
            .boxed();

            let array = expr
                .clone()
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBracket), just(Token::RBracket))
                .map(Expr::Array)
                .boxed();

            let hash = member_name_parser
                .clone()
                .or(just(Token::StringStart)
                    .ignore_then(select! { Token::StringText(s) => s }.or_not())
                    .then_ignore(just(Token::StringEnd))
                    .map(|s| s.unwrap_or_default()))
                .then_ignore(just(Token::Colon))
                .then(expr.clone())
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace))
                .map(Expr::Hash)
                .boxed();

            let if_expr = just(Token::If)
                .ignore_then(expr.clone())
                .then_ignore(just(Token::Then).or_not())
                .then(block_body.clone())
                .then(
                    just(Token::Elsif)
                        .ignore_then(expr.clone())
                        .then_ignore(just(Token::Then).or_not())
                        .then(block_body.clone())
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .then(just(Token::Else).ignore_then(block_body.clone()).or_not())
                .then_ignore(just(Token::End))
                .map(
                    |(((condition, then_branch), elsif_branches), else_branch)| Expr::If {
                        condition: Box::new(condition),
                        then_branch,
                        elsif_branches,
                        else_branch,
                    },
                )
                .boxed();

            let case_expr = just(Token::Case)
                .ignore_then(expr.clone().or_not())
                .then(
                    just(Token::When)
                        .ignore_then(
                            expr.clone()
                                .separated_by(just(Token::Comma))
                                .collect::<Vec<_>>(),
                        )
                        .then(block_body.clone())
                        .map(|(values, body)| CaseClause { values, body })
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .then(just(Token::Else).ignore_then(block_body.clone()).or_not())
                .then_ignore(just(Token::End))
                .map(|((target, clauses), else_expr)| {
                    let target = target as Option<Expr>;
                    Expr::Case {
                        target: Box::new(target.unwrap_or(Expr::Literal(Value::Bool(true)))),
                        clauses,
                        else_expr,
                    }
                })
                .boxed();

            let atom = choice((
                literal,
                variable,
                array,
                hash,
                if_expr,
                case_expr,
                block_lit.clone(),
                expr.clone()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            ))
            .boxed();

            let arg_list = choice((
                member_name_parser
                    .clone()
                    .then_ignore(just(Token::Colon))
                    .then(expr.clone())
                    .map(|(name, val)| (Some(name), val)),
                expr.clone().map(|val| (None, val)),
            ))
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .boxed();

            let call_or_index_or_member = atom
                .clone()
                .foldl(
                    choice((
                        arg_list
                            .clone()
                            .delimited_by(just(Token::LParen), just(Token::RParen))
                            .then(block_lit.clone().or_not())
                            .map(|(args, block)| ("call", args, String::new(), block)),
                        expr.clone()
                            .delimited_by(just(Token::LBracket), just(Token::RBracket))
                            .map(|idx| ("index", vec![(None, idx)], String::new(), None)),
                        just(Token::Dot)
                            .ignore_then(member_name_parser.clone())
                            .then(
                                arg_list
                                    .clone()
                                    .delimited_by(just(Token::LParen), just(Token::RParen))
                                    .or_not(),
                            )
                            .then(block_lit.clone().or_not())
                            .map(|((name, args), block)| {
                                (
                                    "member",
                                    args.unwrap_or_default() as Vec<(Option<String>, Expr)>,
                                    name,
                                    block,
                                )
                            }),
                        just(Token::ColonColon)
                            .ignore_then(member_name_parser.clone())
                            .map(|name| {
                                ("member", vec![] as Vec<(Option<String>, Expr)>, name, None)
                            }),
                    ))
                    .repeated(),
                    |lhs, (kind, args_with_names, name, block)| {
                        let mut args = Vec::new();
                        let mut kwargs = Vec::new();
                        for (name_opt, val) in args_with_names {
                            if let Some(n) = name_opt {
                                kwargs.push((n, val));
                            } else {
                                args.push(val);
                            }
                        }

                        match kind {
                            "call" => {
                                if let Expr::Variable(func_name) = lhs {
                                    Expr::Call {
                                        func: func_name,
                                        args,
                                        kwargs,
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
                                kwargs,
                                block: block.map(Box::new),
                            },
                            _ => unreachable!(),
                        }
                    },
                )
                .boxed();

            let unary = just(Token::Minus)
                .to(UnaryOp::Neg)
                .or(just(Token::Not).to(UnaryOp::Not))
                .repeated()
                .foldr(call_or_index_or_member, |op, expr| Expr::Unary {
                    op,
                    expr: Box::new(expr),
                })
                .boxed();

            let mul_div_mod = unary
                .clone()
                .foldl(
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
                )
                .boxed();

            let add_sub = mul_div_mod
                .clone()
                .foldl(
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
                )
                .boxed();

            let range = add_sub
                .clone()
                .foldl(
                    just(Token::DotDot)
                        .to(BinaryOp::Range)
                        .then(add_sub.clone())
                        .repeated(),
                    |lhs, (op, rhs)| Expr::Binary {
                        left: Box::new(lhs),
                        op,
                        right: Box::new(rhs),
                    },
                )
                .boxed();

            let comparison = range
                .clone()
                .foldl(
                    choice((
                        just(Token::Eq).to(BinaryOp::Eq),
                        just(Token::NotEq).to(BinaryOp::NotEq),
                        just(Token::Lt).to(BinaryOp::Lt),
                        just(Token::LtEq).to(BinaryOp::LtEq),
                        just(Token::Gt).to(BinaryOp::Gt),
                        just(Token::GtEq).to(BinaryOp::GtEq),
                    ))
                    .then(range.clone())
                    .repeated(),
                    |lhs, (op, rhs)| Expr::Binary {
                        left: Box::new(lhs),
                        op,
                        right: Box::new(rhs),
                    },
                )
                .boxed();

            let logical_and = comparison
                .clone()
                .foldl(
                    just(Token::And)
                        .to(BinaryOp::And)
                        .then(comparison.clone())
                        .repeated(),
                    |lhs, (op, rhs)| Expr::Binary {
                        left: Box::new(lhs),
                        op,
                        right: Box::new(rhs),
                    },
                )
                .boxed();

            let logical_or = logical_and
                .clone()
                .foldl(
                    just(Token::Or)
                        .to(BinaryOp::Or)
                        .then(logical_and.clone())
                        .repeated(),
                    |lhs, (op, rhs)| Expr::Binary {
                        left: Box::new(lhs),
                        op,
                        right: Box::new(rhs),
                    },
                )
                .boxed();

            logical_or.boxed()
        });

        let if_stmt = just(Token::If)
            .ignore_then(expr.clone())
            .then_ignore(just(Token::Then).or_not())
            .then(block_body.clone())
            .then(
                just(Token::Elsif)
                    .ignore_then(expr.clone())
                    .then_ignore(just(Token::Then).or_not())
                    .then(block_body.clone())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .then(just(Token::Else).ignore_then(block_body.clone()).or_not())
            .then_ignore(just(Token::End))
            .map(
                |(((condition, then_branch), elsif_branches), else_branch)| Stmt::If {
                    condition,
                    then_branch,
                    elsif_branches,
                    else_branch,
                },
            )
            .boxed();

        let while_stmt = just(Token::While)
            .ignore_then(expr.clone())
            .then(block_body.clone())
            .then_ignore(just(Token::End))
            .map(|(condition, body)| Stmt::While { condition, body })
            .boxed();

        let until_stmt = just(Token::Until)
            .ignore_then(expr.clone())
            .then(block_body.clone())
            .then_ignore(just(Token::End))
            .map(|(condition, body)| Stmt::Until { condition, body })
            .boxed();

        let for_stmt = just(Token::For)
            .ignore_then(name_parser.clone())
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .then(block_body.clone())
            .then_ignore(just(Token::End))
            .map(|((var, iterable), body)| Stmt::For {
                var,
                iterable,
                body,
            })
            .boxed();

        let def_stmt = just(Token::Private)
            .or_not()
            .then_ignore(just(Token::Def))
            .then(choice((
                just(Token::SelfToken)
                    .ignore_then(just(Token::Dot))
                    .ignore_then(member_name_parser.clone())
                    .map(|name| (name, true)),
                name_parser.clone().map(|name| (name, false)),
            )))
            .then(choice((
                select! { Token::Ident(name) => (name, false), Token::Ivar(name) => (name, true) }
                    .then(
                        just(Token::Colon)
                            .ignore_then(type_expr_parser.clone())
                            .or_not(),
                    )
                    .map(|((name, is_ivar), param_type)| Param {
                        name,
                        is_ivar,
                        param_type,
                    })
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
                empty().to(vec![]),
            )))
            .then(
                just(Token::Minus)
                    .ignore_then(just(Token::Gt))
                    .ignore_then(type_expr_parser.clone())
                    .or_not(),
            )
            .then(block_body.clone())
            .then_ignore(just(Token::End))
            .map(
                |((((is_private, (name, is_class_method)), params), return_type), body)| {
                    let is_private = is_private as Option<Token>;
                    Stmt::Function(FunctionStmt {
                        name,
                        params,
                        return_type,
                        body,
                        is_class_method,
                        is_private: is_private.is_some(),
                    })
                },
            )
            .boxed();

        let enum_stmt = just(Token::Enum)
            .ignore_then(name_parser.clone())
            .then(
                select! { Token::Ident(name) => EnumMember { name } }
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .then_ignore(just(Token::End))
            .map(|(name, members)| Stmt::EnumDef { name, members })
            .boxed();

        let class_stmt = just(Token::Class)
            .ignore_then(name_parser.clone())
            .then(block_body.clone())
            .then_ignore(just(Token::End))
            .map(|(name, body)| Stmt::ClassDef { name, body })
            .boxed();

        let property_stmt = choice((
            just(Token::Property).to(PropertyKind::Property),
            just(Token::Getter).to(PropertyKind::Getter),
            just(Token::Setter).to(PropertyKind::Setter),
        ))
        .then(
            name_parser
                .clone()
                .separated_by(just(Token::Comma))
                .collect::<Vec<_>>(),
        )
        .map(|(kind, names)| Stmt::PropertyDecl { names, kind })
        .boxed();

        let assignment = choice((
            // Member Assignment obj.prop = val
            // To avoid ambiguity with Index Assignment, we try Member first
            name_parser
                .clone()
                .then_ignore(just(Token::Dot))
                .then(member_name_parser.clone())
                .then_ignore(just(Token::Assign))
                .then(expr.clone())
                .map(|((receiver, method), value)| {
                    Stmt::Expression(Expr::Member {
                        receiver: Box::new(Expr::Variable(receiver)),
                        method: format!("{}=", method),
                        args: vec![value],
                        kwargs: vec![],
                        block: None,
                    })
                }),
            // Index Assignment: obj[idx] = val
            // We use atom.clone() here to match the receiver properly
            name_parser
                .clone()
                .then(
                    expr.clone()
                        .delimited_by(just(Token::LBracket), just(Token::RBracket)),
                )
                .then_ignore(just(Token::Assign))
                .then(expr.clone())
                .map(|((receiver, index), value)| {
                    Stmt::Expression(Expr::Member {
                        receiver: Box::new(Expr::Variable(receiver)),
                        method: "[]=".to_string(),
                        args: vec![index, value],
                        kwargs: vec![],
                        block: None,
                    })
                }),
            name_parser
                .clone()
                .then_ignore(just(Token::Assign))
                .then(expr.clone())
                .map(|(name, value)| Stmt::Assignment { name, value }),
            select! { Token::Ivar(name) => name }
                .then_ignore(just(Token::Assign))
                .then(expr.clone())
                .map(|(name, value)| Stmt::IvarAssignment { name, value }),
            select! { Token::Cvar(name) => name }
                .then_ignore(just(Token::Assign))
                .then(expr.clone())
                .map(|(name, value)| Stmt::CvarAssignment { name, value }),
        ))
        .boxed();

        let break_stmt = just(Token::Break).to(Stmt::Break).boxed();
        let next_stmt = just(Token::Next).to(Stmt::Next).boxed();

        let return_stmt = just(Token::Return)
            .ignore_then(expr.clone().or_not())
            .map(Stmt::Return)
            .boxed();

        let assert_stmt = just(Token::Assert)
            .ignore_then(expr.clone())
            .then(just(Token::Comma).ignore_then(expr.clone()).or_not())
            .map(|(condition, message)| Stmt::Assert { condition, message })
            .boxed();

        let begin_stmt = just(Token::Begin)
            .ignore_then(block_body.clone())
            .then(
                just(Token::Rescue)
                    .ignore_then(
                        type_expr_parser
                            .clone()
                            .separated_by(just(Token::Comma))
                            .allow_trailing()
                            .collect::<Vec<_>>()
                            .delimited_by(just(Token::LParen), just(Token::RParen))
                            .or_not(),
                    )
                    .then(block_body.clone())
                    .map(|(types, body)| RescueClause {
                        types: types.unwrap_or_default(),
                        body,
                    })
                    .or_not(),
            )
            .then(just(Token::Ensure).ignore_then(block_body.clone()).or_not())
            .then_ignore(just(Token::End))
            .map(|((body, rescue), ensure)| Stmt::Try {
                body,
                rescue,
                ensure,
            })
            .boxed();

        choice((
            if_stmt,
            while_stmt,
            until_stmt,
            for_stmt,
            def_stmt,
            enum_stmt,
            class_stmt,
            property_stmt,
            return_stmt,
            assert_stmt,
            break_stmt,
            next_stmt,
            begin_stmt,
            assignment,
            expr.clone().map(Stmt::Expression),
        ))
    })
    .boxed()
}
