use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Value),
    Variable(String),
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Call {
        func: String,
        args: Vec<Expr>,
        kwargs: Vec<(String, Expr)>,
    },
    Member {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    Array(Vec<Expr>),
    Hash(Vec<(String, Expr)>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Modulo,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    Index,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Expression(Expr),
    Assignment {
        name: String,
        value: Expr,
    },
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        elsif_branches: Vec<(Expr, Vec<Stmt>)>,
        else_branch: Option<Vec<Stmt>>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
}
