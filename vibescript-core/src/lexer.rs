use logos::Logos;
use std::fmt;

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t\n\f]+")] // Skip whitespace
#[logos(skip(r"#[^\n]*", allow_greedy = true))] // Skip comments
pub enum Token {
    #[token("def")]
    Def,
    #[token("do")]
    Do,
    #[token("if")]
    If,
    #[token("elsif")]
    Elsif,
    #[token("else")]
    Else,
    #[token("end")]
    End,
    #[token("while")]
    While,
    #[token("until")]
    Until,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("break")]
    Break,
    #[token("next")]
    Next,
    #[token("case")]
    Case,
    #[token("when")]
    When,
    #[token("begin")]
    Begin,
    #[token("rescue")]
    Rescue,
    #[token("ensure")]
    Ensure,
    #[token("enum")]
    Enum,
    #[token("class")]
    Class,
    #[token("self")]
    SelfToken,
    #[token("property")]
    Property,
    #[token("getter")]
    Getter,
    #[token("setter")]
    Setter,
    #[token("private")]
    Private,
    #[token("return")]
    Return,
    #[token("assert")]
    Assert,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("nil")]
    Nil,

    #[token("..")]
    DotDot,

    #[regex(":[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    Symbol(String),

    #[regex("[a-zA-Z_][a-zA-Z0-9_]*[\\?!]?", |lex| lex.slice().to_string())]
    Ident(String),

    #[regex("@[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    Ivar(String),

    #[regex("@@[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[2..].to_string())]
    Cvar(String),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Int(i64),

    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    Float(f64),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        let content = &s[1..s.len()-1];
        Some(content.replace("\\\"", "\"").replace("\\n", "\n").replace("\\t", "\t"))
    })]
    String(String),

    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("=")]
    Assign,
    #[token("==")]
    Eq,
    #[token("!=")]
    NotEq,
    #[token("<")]
    Lt,
    #[token("<=")]
    LtEq,
    #[token(">")]
    Gt,
    #[token(">=")]
    GtEq,
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("!")]
    Not,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("::")]
    ColonColon,
    #[token(".")]
    Dot,
    #[token("|")]
    Pipe,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Token::Def => write!(f, "def"),
            Token::Do => write!(f, "do"),
            Token::If => write!(f, "if"),
            Token::Elsif => write!(f, "elsif"),
            Token::Else => write!(f, "else"),
            Token::End => write!(f, "end"),
            Token::While => write!(f, "while"),
            Token::Until => write!(f, "until"),
            Token::For => write!(f, "for"),
            Token::In => write!(f, "in"),
            Token::Break => write!(f, "break"),
            Token::Next => write!(f, "next"),
            Token::Case => write!(f, "case"),
            Token::When => write!(f, "when"),
            Token::Begin => write!(f, "begin"),
            Token::Rescue => write!(f, "rescue"),
            Token::Ensure => write!(f, "ensure"),
            Token::Enum => write!(f, "enum"),
            Token::Class => write!(f, "class"),
            Token::SelfToken => write!(f, "self"),
            Token::Property => write!(f, "property"),
            Token::Getter => write!(f, "getter"),
            Token::Setter => write!(f, "setter"),
            Token::Private => write!(f, "private"),
            Token::Return => write!(f, "return"),
            Token::Assert => write!(f, "assert"),
            Token::True => write!(f, "true"),
            Token::False => write!(f, "false"),
            Token::Nil => write!(f, "nil"),
            Token::DotDot => write!(f, ".."),
            Token::Symbol(s) => write!(f, ":{}", s),
            Token::Ident(s) => write!(f, "{}", s),
            Token::Ivar(s) => write!(f, "@{}", s),
            Token::Cvar(s) => write!(f, "@@{}", s),
            Token::Int(i) => write!(f, "{}", i),
            Token::Float(fl) => write!(f, "{}", fl),
            Token::String(s) => write!(f, "\"{}\"", s),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::Assign => write!(f, "="),
            Token::Eq => write!(f, "=="),
            Token::NotEq => write!(f, "!="),
            Token::Lt => write!(f, "<"),
            Token::LtEq => write!(f, "<="),
            Token::Gt => write!(f, ">"),
            Token::GtEq => write!(f, ">="),
            Token::And => write!(f, "&&"),
            Token::Or => write!(f, "||"),
            Token::Not => write!(f, "!"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::Comma => write!(f, ","),
            Token::Colon => write!(f, ":"),
            Token::ColonColon => write!(f, "::"),
            Token::Dot => write!(f, "."),
            Token::Pipe => write!(f, "|"),
        }
    }
}
