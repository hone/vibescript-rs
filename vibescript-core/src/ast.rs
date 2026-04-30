use crate::value::{EnumMember, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Value),
    Variable(String),
    InstanceVar(String),
    ClassVar(String),
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
        kwargs: Vec<(String, Expr)>,
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
    If {
        condition: Box<Expr>,
        then_branch: Vec<Stmt>,
        elsif_branches: Vec<(Expr, Vec<Stmt>)>,
        else_branch: Option<Vec<Stmt>>,
    },
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
    Range,
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
    IvarAssignment {
        name: String,
        value: Expr,
    },
    CvarAssignment {
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
    Function(FunctionStmt),
    EnumDef {
        name: String,
        members: Vec<EnumMember>,
    },
    ClassDef {
        name: String,
        body: Vec<Stmt>,
    },
    PropertyDecl {
        names: Vec<String>,
        kind: PropertyKind,
    },
    Return(Option<Expr>),
    Assert {
        condition: Expr,
        message: Option<Expr>,
    },
    Try {
        body: Vec<Stmt>,
        rescue: Option<RescueClause>,
        ensure: Option<Vec<Stmt>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PropertyKind {
    Property,
    Getter,
    Setter,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeKind {
    Any,
    Int,
    Float,
    Number,
    String,
    Bool,
    Nil,
    Duration,
    Time,
    Money,
    Array,
    Hash,
    Shape,
    Union,
    Enum,
    Function,
    Object,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeExpr {
    pub name: String,
    pub kind: TypeKind,
    pub nullable: bool,
    pub type_args: Vec<TypeExpr>,
    pub shape: Vec<(String, TypeExpr)>, // Changed from HashMap to Vec for Hash/Eq and stable iteration
    pub union_types: Vec<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionStmt {
    pub name: String,
    pub params: Vec<crate::value::Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Vec<Stmt>,
    pub is_class_method: bool,
    pub is_private: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RescueClause {
    pub types: Vec<TypeExpr>,
    pub body: Vec<Stmt>,
}
