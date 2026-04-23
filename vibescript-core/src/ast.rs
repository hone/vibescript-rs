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
        block: Option<Box<Expr>>,
    },
    Member {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Expr>,
        block: Option<Box<Expr>>,
    },
    Array(Vec<Expr>),
    Hash(Vec<(String, Expr)>),
    Case {
        target: Box<Expr>,
        clauses: Vec<CaseClause>,
        else_expr: Option<Vec<Stmt>>,
    },
    Block {
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    InterpolatedString(Vec<StringPart>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Text(String),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseClause {
    pub values: Vec<Expr>,
    pub body: Vec<Stmt>,
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
    Until {
        condition: Expr,
        body: Vec<Stmt>,
    },
    For {
        var: String,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    Break,
    Next,
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    EnumDef {
        name: String,
        members: Vec<EnumMember>,
    },
    Return(Option<Expr>),
    Try {
        body: Vec<Stmt>,
        rescue: Option<RescueClause>,
        ensure: Option<Vec<Stmt>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumMember {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RescueClause {
    pub types: Vec<String>,
    pub body: Vec<Stmt>,
}
